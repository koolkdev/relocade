mod scattered;

use std::{cell::Cell, marker::PhantomData};

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, Mem, MemoryImport, MemoryInt, Program, Signature, Type, Val,
    I1, I32,
};

const PAGE_SHIFT: u32 = 12;
const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
const PAGE_OFFSET: u32 = PAGE_BYTES - 1;
const FRAME_MASK: u32 = !PAGE_OFFSET;
const PRESENT: u32 = 1;
const WRITABLE: u32 = 2;
// A resolved range carries its first frame and permissions, or the denied page.
// These private flags distinguish scattered backing and a denial on the second page.
const SCATTERED: u32 = 4;
const LATER_DENIAL: u32 = 8;

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

/// Address and error describe the fault only when `condition` is true.
pub(super) struct AccessFault {
    pub(super) condition: Val<I1>,
    pub(super) address: Val<I32>,
    pub(super) error: Val<I32>,
}

pub(super) struct DirectRange {
    pub(super) unavailable: Val<I1>,
    pub(super) physical: Val<I32>,
}

/// The caller must leave the current path on a fault before reading or writing.
/// Resolution checks the complete span without changing guest or CPU state.
pub(super) struct Access<T: MemoryInt> {
    pub(super) fault: AccessFault,
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
        let table = PageTable {
            entries: program.import_memory(MemoryImport {
                module: "wasm86".into(),
                name: "machine".into(),
                minimum: 64,
                maximum: None,
            }),
        };
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

    /// Resolves a positive fixed span of at most one page in length. Such a span
    /// touches at most two pages, so its first and last bytes determine permission.
    fn resolve_range(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Val<I32>, BuildError> {
        assert!((1..=PAGE_BYTES).contains(&bytes));
        let first_entry = self.table.entry(body, start)?;
        // Packed results discard unrelated bits before adding resolution flags.
        let resolution_word = if bytes == 1 {
            first_entry.and(FRAME_MASK | PRESENT | WRITABLE)
        } else {
            let crosses = crosses_page(start, bytes);
            body.if_value::<I32>(
                crosses,
                |mut arm| {
                    let resolution_word = arm.call::<I32>(
                        self.range_resolver,
                        &[
                            start.into(),
                            (bytes - 1).into(),
                            (&first_entry).into(),
                            intent.required_permissions().into(),
                        ],
                    )?;
                    arm.yield_(resolution_word)
                },
                |arm| arm.yield_(first_entry.and(FRAME_MASK | PRESENT | WRITABLE)),
            )?
        };
        Ok(resolution_word)
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

    /// Checks one linear span, rejecting a range past the end of the 32-bit
    /// address space. Instruction fetch implements EIP wrap through byte reads.
    pub(super) fn resolve_access<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        intent: Intent,
    ) -> Result<Access<T>, BuildError> {
        let resolution_word = self.resolve_range(body, start, T::BYTES, intent)?;
        let error = match intent {
            Intent::Write => resolution_word.and(PRESENT).or(intent.base_error_code()),
            // Presence is the only read/fetch permission, so denial is non-present.
            Intent::Read | Intent::Fetch => body.value(intent.base_error_code())?,
        };
        Ok(Access {
            fault: AccessFault {
                condition: resolution_word
                    .and(intent.required_permissions())
                    .ne(intent.required_permissions()),
                address: resolution_word
                    .and(LATER_DENIAL)
                    .ne(0)
                    .select(resolution_word.and(FRAME_MASK), start),
                error,
            },
            linear: start.clone(),
            physical: physical_address(&resolution_word, start),
            scattered: resolution_word.and(SCATTERED).ne(0),
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

#[derive(Clone, Copy)]
struct PageTable {
    entries: Mem,
}

impl PageTable {
    fn entry(
        self,
        body: &mut FunctionBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let index = address.unsigned().shr(PAGE_SHIFT).shl(2);
        body.load_at::<I32>(self.entries, index, 0)
    }

    /// Looks up the second page of a crossing span, reusing the first entry.
    /// The range must touch at most two pages.
    fn lookup_span(
        self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        last_byte_offset: impl Into<Val<I32>>,
        first_entry: &Val<I32>,
    ) -> Result<TwoPageSpan, BuildError> {
        let last_address = start.add(last_byte_offset);
        // Every page-table slot has backing, including a wrapped last index.
        // Looking up either entry is safe before classifying the range.
        let second_entry = self.entry(body, &last_address)?;
        Ok(TwoPageSpan {
            start: start.clone(),
            last_address,
            first_entry: first_entry.clone(),
            second_entry,
        })
    }

    // A separate generated function keeps the last-address calculation in the
    // cross-page path even when scattered accesses also use that address later.
    fn define_range_resolver(self, mut body: FunctionBuilder<'_>) -> Result<(), BuildError> {
        let start = body.parameter::<I32>(0)?;
        let last_byte_offset = body.parameter::<I32>(1)?;
        let first_entry = body.parameter::<I32>(2)?;
        let required_permissions = body.parameter::<I32>(3)?;
        let span = self.lookup_span(&mut body, &start, last_byte_offset, &first_entry)?;
        body.if_(span.access_denied(&required_permissions), |arm| {
            arm.return_(span.encode_denial(&required_permissions))
        })?;
        body.return_(
            first_entry.and(FRAME_MASK | PRESENT | WRITABLE).or(span
                .has_scattered_backing()
                .unsigned()
                .extend::<I32>()
                .shl(2)),
        )
    }
}

/// Page-table facts for a range crossing one page boundary. Shared by the
/// contiguous-access check and detailed resolution; only resolution encodes denials.
struct TwoPageSpan {
    start: Val<I32>,
    last_address: Val<I32>,
    first_entry: Val<I32>,
    second_entry: Val<I32>,
}

impl TwoPageSpan {
    /// Rejects missing permissions or an address-space wrap.
    fn access_denied(&self, required_permissions: impl Into<Val<I32>> + Copy) -> Val<I1> {
        self.first_entry
            .and(&self.second_entry)
            .and(required_permissions)
            .ne(required_permissions)
            .or(self.last_address.unsigned().lt(&self.start))
    }

    fn has_scattered_backing(&self) -> Val<I1> {
        let first_frame = self.first_entry.and(FRAME_MASK);
        let next_frame = first_frame.add(PAGE_BYTES);
        self.second_entry
            .and(FRAME_MASK)
            .ne(&next_frame)
            .or(next_frame.unsigned().lt(&first_frame))
    }

    fn encode_denial(&self, required_permissions: &Val<I32>) -> Val<I32> {
        let second_denial = self
            .last_address
            .and(FRAME_MASK)
            .or(self.second_entry.and(PRESENT))
            .or(LATER_DENIAL);
        let first_denial = self.first_entry.and(FRAME_MASK | PRESENT | WRITABLE);
        let denial = self
            .first_entry
            .and(required_permissions)
            .ne(required_permissions)
            .select(first_denial, second_denial);
        // Range wrap has priority over page permissions and reports a non-present
        // fault at the start. Otherwise the first denied page supplies the error.
        self.last_address
            .unsigned()
            .lt(&self.start)
            .select(0, denial)
    }
}

fn crosses_page(start: &Val<I32>, bytes: u32) -> Val<I1> {
    start.and(PAGE_OFFSET).unsigned().ge(PAGE_BYTES - bytes + 1)
}

fn physical_address(entry: &Val<I32>, address: &Val<I32>) -> Val<I32> {
    entry.and(FRAME_MASK).or(address.and(PAGE_OFFSET))
}

#[cfg(test)]
mod tests;
