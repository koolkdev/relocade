//! Complete byte-span resolution, with optional architectural fault handling.

use super::page_table::{
    crosses_page, physical_address, scattered_backing, FRAME_MASK, LATER_DENIAL, PAGE_BYTES,
    SCATTERED,
};
use super::{Intent, Memory, PageCache};
use crate::exception::Exception;
use wasm86_compiler::{BlockBuilder, BuildError, MemoryInt, Val, I1, I32};

type FaultHandler<'handler> = dyn for<'body> FnMut(BlockBuilder<'body>, Exception<Val<I32>>) -> Result<(), BuildError>
    + 'handler;

/// Resolution of a complete span before any guest transfer. Non-faulting callers
/// must guard transfers with `!denied`; native bulk transfers also need `!scattered`.
#[derive(Clone)]
pub(crate) struct Access {
    pub(super) linear: Val<I32>,
    pub(crate) physical: Val<I32>,
    pub(crate) scattered: Val<I1>,
    pub(crate) denied: Val<I1>,
    /// Denied or scattered: a native contiguous transfer cannot use this span.
    pub(crate) unavailable: Val<I1>,
    pub(super) intent: Intent,
    pub(super) bytes: Option<u32>,
}

impl Access {
    pub(super) fn check_field<T: MemoryInt>(&self, offset: u32) {
        let bytes = self
            .bytes
            .expect("a typed transfer needs a constant checked span");
        assert!(
            offset <= bytes && T::BYTES <= bytes - offset,
            "a transferred field must fit the checked span"
        );
    }
}

impl Memory {
    /// Resolves a complete linear byte span, including wrapping at 2^32.
    /// Segment checks must already have validated the offset span.
    /// A fault handler must terminate the denied path. Without one, denial is
    /// returned to the caller. Faulting spans must be nonempty; a zero-length
    /// non-faulting probe returns denial.
    pub(crate) fn resolve_access(
        &self,
        body: &mut BlockBuilder<'_>,
        start: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
        cache: Option<&mut PageCache>,
        on_fault: Option<&mut FaultHandler<'_>>,
    ) -> Result<Access, BuildError> {
        let bytes = body.value(bytes)?;
        let constant_bytes = body.constant_bits(&bytes)?.map(|bytes| bytes as u32);
        let faulting = on_fault.is_some();
        if faulting {
            assert_ne!(constant_bytes, Some(0), "a faulting span must be nonempty");
        }
        let first_entry = match cache {
            Some(cache) => cache.lookup(self.table, body, start)?,
            None => self.table.entry(body, start)?,
        };
        let required = intent.required_permissions();
        let first_denied = first_entry.and(required).ne(required);
        if !faulting {
            if let Some(bytes) = constant_bytes.filter(|bytes| *bytes <= PAGE_BYTES) {
                let (denied, scattered, unavailable) = if bytes == 0 {
                    (body.value(true)?, body.value(false)?, body.value(true)?)
                } else {
                    body.if_value::<(I1, I1, I1)>(
                        crosses_page(start, bytes),
                        |mut crossing| {
                            let next_entry =
                                self.table.entry(&mut crossing, &start.add(bytes - 1))?;
                            let denied = first_entry.and(&next_entry).and(required).ne(required);
                            let scattered =
                                scattered_backing(&first_entry.and(FRAME_MASK), &next_entry);
                            // Keep the combined result on each arm. Direct consumers
                            // need only this channel, retaining their short probe code.
                            crossing.yield_((
                                &denied,
                                scattered.and(denied.eq(0)),
                                denied.or(&scattered),
                            ))
                        },
                        |single_page| single_page.yield_((&first_denied, false, &first_denied)),
                    )?
                };
                return Ok(Access {
                    linear: start.clone(),
                    physical: physical_address(&first_entry, start),
                    scattered,
                    denied,
                    unavailable,
                    intent,
                    bytes: constant_bytes,
                });
            }
        }
        let report_fault = |fault_body: BlockBuilder<'_>, address: Val<I32>, present: Val<I1>| {
            match on_fault {
                Some(on_fault) => {
                    let error_code = match intent {
                        Intent::Write => present
                            .unsigned()
                            .extend::<I32>()
                            .or(intent.base_error_code()),
                        // Presence is the only read/fetch permission, so denial is non-present.
                        Intent::Read | Intent::Fetch => {
                            fault_body.value(intent.base_error_code())?
                        }
                    };
                    on_fault(
                        fault_body,
                        Exception::PageFault {
                            linear_address: address,
                            error_code,
                        },
                    )
                }
                None => fault_body.yield_((false, physical_address(&first_entry, start), true)),
            }
        };
        let (scattered, physical, denied) = if constant_bytes == Some(1) && faulting {
            // Keep the scalar byte path's single page check and result shape.
            let physical = body.if_value::<I32>(
                &first_denied,
                |fault_body| report_fault(fault_body, start.clone(), first_entry.truncate::<I1>()),
                |allowed| allowed.yield_(physical_address(&first_entry, start)),
            )?;
            (body.value(false)?, physical, body.value(false)?)
        } else {
            // Success exits the outer block with (scattered, physical, denied).
            // Denial exits the inner block with (address, present).
            body.block::<(I1, I32, I1)>(|mut access_body, success| {
                let (address, present) = access_body.block::<(I32, I1)>(|mut checks, fault| {
                    let resolve_pages = |mut crossing: BlockBuilder<'_>| {
                        let resolution_word = crossing.call::<I32>(
                            self.range_resolver,
                            &[
                                start.into(),
                                bytes.sub(1).into(),
                                (&first_entry).into(),
                                required.into(),
                            ],
                        )?;
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
                                false,
                            ),
                        )
                    };
                    match constant_bytes {
                        Some(0) => checks.branch(&fault, (start, first_entry.truncate::<I1>())),
                        Some(bytes) if bytes <= PAGE_BYTES => {
                            if bytes > 1 {
                                // Aligned power-of-two operands fit in one page.
                                let may_cross = if bytes.is_power_of_two() {
                                    start.and(bytes - 1).ne(0)
                                } else {
                                    true.into()
                                };
                                checks.if_(may_cross, |mut candidate| {
                                    candidate.if_(crosses_page(start, bytes), resolve_pages)
                                })?;
                            }
                            checks.branch_if(
                                &first_denied,
                                &fault,
                                (start, first_entry.truncate::<I1>()),
                            )?;
                            checks.branch(
                                &success,
                                (false, physical_address(&first_entry, start), false),
                            )
                        }
                        _ => {
                            checks.branch_if(
                                bytes.eq(0),
                                &fault,
                                (start, first_entry.truncate::<I1>()),
                            )?;
                            resolve_pages(checks)
                        }
                    }
                })?;
                report_fault(access_body, address, present)
            })?
        };
        Ok(Access {
            linear: start.clone(),
            physical,
            unavailable: denied.or(&scattered),
            scattered,
            denied,
            intent,
            bytes: constant_bytes,
        })
    }
}
