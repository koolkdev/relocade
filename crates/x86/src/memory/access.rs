//! Checked byte spans separate access permissions from the types of their fields.

use super::page_table::{
    crosses_page, physical_address, FRAME_MASK, LATER_DENIAL, PAGE_BYTES, SCATTERED,
};
use super::{Intent, Memory, PageCache};
use crate::exception::Exception;
use wasm86_compiler::{BuildError, FunctionBuilder, MemoryInt, Val, I1, I32};

pub(crate) struct DirectRange {
    pub(crate) unavailable: Val<I1>,
    pub(crate) physical: Val<I32>,
}

/// A complete span whose permissions have been checked before any guest transfer.
pub(crate) struct Access {
    pub(super) linear: Val<I32>,
    pub(super) physical: Val<I32>,
    pub(super) scattered: Val<I1>,
    pub(super) intent: Intent,
    pub(super) bytes: u32,
}

impl Access {
    pub(super) fn check_field<T: MemoryInt>(&self, offset: u32) {
        assert!(
            offset <= self.bytes && T::BYTES <= self.bytes - offset,
            "a transferred field must fit the checked span"
        );
    }
}

impl Memory {
    /// Proves that the complete span permits the requested access and has
    /// contiguous physical backing. Failure does not raise an architectural fault.
    pub(crate) fn check_direct_access(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
        intent: Intent,
        cache: Option<&mut PageCache>,
    ) -> Result<DirectRange, BuildError> {
        assert!((1..=PAGE_BYTES).contains(&bytes));
        let first_entry = match cache {
            Some(cache) => cache.lookup(self.table, body, start)?,
            None => self.table.entry(body, start)?,
        };
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

    /// Checks a complete linear byte span, including ranges
    /// wrapping at 2^32. Segment checks must already have validated the offset span.
    /// The callback must exit the fault path; successful paths yield an `Access`
    /// for the caller's read or write.
    pub(crate) fn resolve_access(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
        intent: Intent,
        on_fault: impl FnOnce(FunctionBuilder<'_>, Exception<Val<I32>>) -> Result<(), BuildError>,
    ) -> Result<Access, BuildError> {
        assert!((1..=PAGE_BYTES).contains(&bytes));
        let first_entry = self.table.entry(body, start)?;
        let required = intent.required_permissions();
        let first_denied = first_entry.and(required).ne(required);
        let report_fault =
            |fault_body: FunctionBuilder<'_>, address: Val<I32>, present: Val<I1>| {
                let error_code = match intent {
                    Intent::Write => present
                        .unsigned()
                        .extend::<I32>()
                        .or(intent.base_error_code()),
                    // Presence is the only read/fetch permission, so denial is non-present.
                    Intent::Read | Intent::Fetch => fault_body.value(intent.base_error_code())?,
                };
                on_fault(
                    fault_body,
                    Exception::PageFault {
                        linear_address: address,
                        error_code,
                    },
                )
            };
        let (scattered, physical) = if bytes == 1 {
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
                    // Power-of-two spans divide the page size, so aligned accesses
                    // fit. Other spans, including six-byte pointers, can always cross.
                    let may_cross = if bytes.is_power_of_two() {
                        start.and(bytes - 1).ne(0)
                    } else {
                        true.into()
                    };
                    checks.if_(may_cross, |mut candidate| {
                        candidate.if_(crosses_page(start, bytes), |mut crossing| {
                            // Crossing access: check both pages and resolve their backing.
                            let resolution_word = crossing.call::<I32>(
                                self.range_resolver,
                                &[
                                    start.into(),
                                    (bytes - 1).into(),
                                    (&first_entry).into(),
                                    required.into(),
                                ],
                            )?;
                            // Report start, or the next page's first byte if it was denied.
                            crossing.branch_if(
                                resolution_word.and(required).ne(required),
                                &fault,
                                (
                                    resolution_word
                                        .and(LATER_DENIAL)
                                        .ne(0)
                                        .select(resolution_word.and(FRAME_MASK), start),
                                    resolution_word.truncate::<I1>(),
                                ),
                            )?;
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
                    checks.branch_if(
                        &first_denied,
                        &fault,
                        (start, first_entry.truncate::<I1>()),
                    )?;
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
            bytes,
        })
    }
}
