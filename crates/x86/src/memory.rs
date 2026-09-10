mod page_table;
mod scattered;

use std::{cell::Cell, marker::PhantomData};

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, Mem, MemoryImport, MemoryInt, Program, Signature, Type, Val,
    I1, I32,
};

use page_table::{
    crosses_page, physical_address, PageTable, FRAME_MASK, LATER_DENIAL, PAGE_BYTES, PRESENT,
    SCATTERED, WRITABLE,
};

/// Owns generated access helpers for one module. Frontends discard this owner
/// and its program together when construction fails.
pub(super) struct Memory {
    guest: Mem,
    table: PageTable,
    range_resolver: Func,
    scattered_readers: [Cell<Option<Func>>; 3],
    scattered_writers: [Cell<Option<Func>>; 3],
}

#[derive(Clone, Copy)]
pub(super) enum Intent {
    Fetch,
    Read,
    Write,
}

impl Intent {
    fn required_permissions(self) -> u32 {
        match self {
            Self::Fetch | Self::Read => PRESENT,
            Self::Write => PRESENT | WRITABLE,
        }
    }

    fn base_error_code(self) -> u32 {
        match self {
            Self::Fetch => 16,
            Self::Read => 0,
            Self::Write => 2,
        }
    }
}

/// Fault details supplied only inside the denied access path.
pub(super) struct AccessFault {
    pub(super) address: Val<I32>,
    pub(super) error: Val<I32>,
}

pub(super) struct DirectRange {
    pub(super) unavailable: Val<I1>,
    pub(super) physical: Val<I32>,
}

/// A complete span whose permissions have been checked before any guest transfer.
pub(super) struct Access<T: MemoryInt> {
    linear: Val<I32>,
    physical: Val<I32>,
    scattered: Val<I1>,
    intent: Intent,
    ty: PhantomData<T>,
}

impl Memory {
    pub(super) fn declare(program: &mut Program) -> Result<Self, BuildError> {
        let guest = program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "guest".into(),
            minimum: 1,
            maximum: None,
        });
        let table = PageTable::declare(program);
        let range_resolver = program.function(
            Signature {
                parameters: vec![Type::I32, Type::I32, Type::I32, Type::I32],
                result: Some(Type::I32),
            },
            |body| table.define_range_resolver(body),
        )?;
        Ok(Self {
            guest,
            table,
            range_resolver,
            scattered_readers: std::array::from_fn(|_| Cell::new(None)),
            scattered_writers: std::array::from_fn(|_| Cell::new(None)),
        })
    }

    /// Proves that the complete span permits the requested access and has
    /// contiguous physical backing. Failure does not raise an architectural fault.
    pub(super) fn check_direct_access(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<DirectRange, BuildError> {
        assert!((1..=PAGE_BYTES).contains(&bytes));
        let first_entry = self.table.entry(body, start)?;
        let unavailable = body.if_value::<I1>(
            crosses_page(start, bytes),
            |mut arm| {
                let span = self
                    .table
                    .lookup_span(&mut arm, start, bytes - 1, &first_entry)?;
                arm.yield_(
                    span.access_denied(intent.required_permissions())
                        .or(span.has_scattered_backing()),
                )
            },
            |arm| {
                arm.yield_(
                    first_entry
                        .and(intent.required_permissions())
                        .ne(intent.required_permissions()),
                )
            },
        )?;
        Ok(DirectRange {
            unavailable,
            physical: physical_address(&first_entry, start),
        })
    }

    /// Checks exactly `T::BYTES` bytes (1, 2, 4 or 8), rejecting address-space wrap.
    /// Instruction fetch handles EIP wrap through separate byte reads.
    /// The callback must exit the fault path; successful paths yield an `Access`
    /// for the caller's read or write.
    pub(super) fn resolve_access<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        intent: Intent,
        on_fault: impl FnOnce(FunctionBuilder<'_>, AccessFault) -> Result<(), BuildError>,
    ) -> Result<Access<T>, BuildError> {
        let first_entry = self.table.entry(body, start)?;
        let required = intent.required_permissions();
        let first_denied = first_entry.and(required).ne(required);
        let report_fault =
            |fault_body: FunctionBuilder<'_>, address: Val<I32>, present: Val<I1>| {
                let error = match intent {
                    Intent::Write => present
                        .unsigned()
                        .extend::<I32>()
                        .or(intent.base_error_code()),
                    // Presence is the only read/fetch permission, so denial is non-present.
                    Intent::Read | Intent::Fetch => fault_body.value(intent.base_error_code())?,
                };
                on_fault(fault_body, AccessFault { address, error })
            };
        let (scattered, physical) = if T::BYTES == 1 {
            // A byte needs only the first page check.
            let physical = body.if_value::<I32>(
                &first_denied,
                |fault_body| report_fault(fault_body, start.clone(), first_entry.truncate::<I1>()),
                |allowed| allowed.yield_(physical_address(&first_entry, start)),
            )?;
            (body.value(false)?, physical)
        } else {
            // Success exits the outer block with (scattered, physical). Fault exits
            // the inner block with (address, present) and reaches the handler below.
            body.block::<(I1, I32)>(|mut access_body, success| {
                let (address, present) = access_body.block::<(I32, I1)>(|mut checks, fault| {
                    // Aligned accesses fit one page: each supported width divides
                    // the page size. Only unaligned accesses need a crossing check.
                    checks.if_(start.and(T::BYTES - 1).ne(0), |mut unaligned| {
                        unaligned.if_(crosses_page(start, T::BYTES), |mut crossing| {
                            // Crossing access: check both pages and resolve their backing.
                            let resolution_word = crossing.call::<I32>(
                                self.range_resolver,
                                &[
                                    start.into(),
                                    (T::BYTES - 1).into(),
                                    (&first_entry).into(),
                                    required.into(),
                                ],
                            )?;
                            crossing.if_(resolution_word.and(required).ne(required), |denied| {
                                // Report start, or the next page's first byte if it was denied.
                                denied.branch(
                                    &fault,
                                    (
                                        resolution_word
                                            .and(LATER_DENIAL)
                                            .ne(0)
                                            .select(resolution_word.and(FRAME_MASK), start),
                                        resolution_word.truncate::<I1>(),
                                    ),
                                )
                            })?;
                            crossing.branch(
                                &success,
                                (
                                    resolution_word.and(SCATTERED).ne(0),
                                    physical_address(&resolution_word, start),
                                ),
                            )
                        })
                    })?;
                    // Single-page access: use the first page's permissions and frame.
                    checks.if_(&first_denied, |denied| {
                        denied.branch(&fault, (start, first_entry.truncate::<I1>()))
                    })?;
                    checks.branch(&success, (false, physical_address(&first_entry, start)))
                })?;
                // All wider-access faults meet here.
                report_fault(access_body, address, present)
            })?
        };
        Ok(Access {
            linear: start.clone(),
            physical,
            scattered,
            intent,
            ty: PhantomData,
        })
    }

    pub(super) fn read<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        access: &Access<T>,
    ) -> Result<Val<T>, BuildError> {
        if T::BYTES == 1 {
            return self.load(body, &access.physical, 0);
        }
        body.if_value::<T>(
            &access.scattered,
            |mut arm| {
                let reader = self.scattered_reader::<T>(arm.program())?;
                let value = arm.call::<T>(reader, &[(&access.linear).into()])?;
                arm.yield_(value)
            },
            |mut arm| {
                let value = self.load::<T>(&mut arm, &access.physical, 0)?;
                arm.yield_(value)
            },
        )
    }

    pub(super) fn write<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        access: &Access<T>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        assert!(
            matches!(access.intent, Intent::Write),
            "store requires a write access"
        );
        if T::BYTES == 1 {
            return body.store_at::<T>(self.guest, &access.physical, 0, value);
        }
        body.if_else(
            &access.scattered,
            |mut arm| {
                let writer = self.scattered_writer::<T>(arm.program())?;
                arm.call_void(writer, &[(&access.linear).into(), value.into()])
            },
            |mut arm| arm.store_at::<T>(self.guest, &access.physical, 0, value),
        )
    }

    /// The caller must prove this entire read is present and physically contiguous.
    pub(super) fn load<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        physical: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.guest, physical, offset)
    }
}

#[cfg(test)]
mod tests;
