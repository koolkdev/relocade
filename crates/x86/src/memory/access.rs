//! Complete byte-span resolution, with optional architectural fault handling.

use super::page_table::{
    crosses_page, physical_address, scattered_backing, FRAME_MASK, LATER_DENIAL, PAGE_BYTES,
    SCATTERED,
};
use super::{Intent, PageCache, VirtualMemory};
use crate::exception::Exception;
use wasm86_compiler::{BlockBuilder, BuildError, MemoryInt, Val, I1, I32};

pub(super) type FaultHandler<'handler> = dyn for<'body> FnMut(BlockBuilder<'body>, Exception<Val<I32>>) -> Result<(), BuildError>
    + 'handler;

/// Resolution of a complete span before any guest transfer. Denied spans are
/// always unavailable for direct transfer. Otherwise, `unavailable` identifies
/// scattered virtual backing or physical routing resolved at transfer time.
/// Non-faulting callers must guard transfers with `!denied`.
#[derive(Clone)]
pub(crate) struct Access {
    pub(crate) linear: Val<I32>,
    pub(crate) physical: Val<I32>,
    pub(crate) denied: Val<I1>,
    pub(crate) unavailable: Val<I1>,
    pub(crate) watched: Val<I1>,
    pub(crate) intent: Intent,
    pub(crate) constant_bytes: Option<u32>,
}

impl Access {
    /// Permitted spans that need scattered transfer. Denied spans have no
    /// transferable backing classification.
    pub(super) fn scattered(&self) -> Val<I1> {
        self.unavailable.and(self.denied.eq(0))
    }

    pub(super) fn check_field<T: MemoryInt>(&self, offset: u32) {
        let bytes = self
            .constant_bytes
            .expect("a typed transfer needs a constant checked span");
        assert!(
            offset <= bytes && T::BYTES <= bytes - offset,
            "a transferred field must fit the checked span"
        );
    }
}

impl VirtualMemory {
    /// Resolves a complete linear byte span, including wrapping at 2^32.
    /// Segment validation and its fault precedence belong to the caller.
    /// A fault handler must terminate the denied path. Without one, denial is
    /// returned to the caller without raising an architectural fault. Faulting
    /// spans must be nonempty; an empty non-faulting probe returns denial.
    pub(crate) fn resolve_access(
        &self,
        body: &mut BlockBuilder<'_>,
        start: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
        cache: Option<&mut PageCache>,
        mut on_fault: Option<&mut FaultHandler<'_>>,
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
        let watch_mask = if self.code.is_some() && matches!(intent, Intent::Write) {
            super::CODE_WATCH
        } else {
            0
        };
        let first_watched = first_entry.and(watch_mask).ne(0);
        if !faulting {
            if let Some(bytes) = constant_bytes.filter(|bytes| *bytes <= PAGE_BYTES) {
                let (denied, unavailable, watched) = if bytes == 0 {
                    (body.value(true)?, body.value(true)?, body.value(false)?)
                } else {
                    body.if_value::<(I1, I1, I1)>(
                        crosses_page(start, bytes),
                        |mut crossing| {
                            let next_entry =
                                self.table.entry(&mut crossing, &start.add(bytes - 1))?;
                            let denied = first_entry.and(&next_entry).and(required).ne(required);
                            let scattered =
                                scattered_backing(&first_entry.and(FRAME_MASK), &next_entry);
                            // Direct-only consumers can discard the separate denial result.
                            crossing.yield_((
                                &denied,
                                denied.or(scattered),
                                first_entry.or(&next_entry).and(watch_mask).ne(0),
                            ))
                        },
                        |single_page| {
                            single_page.yield_((&first_denied, &first_denied, &first_watched))
                        },
                    )?
                };
                return Ok(Access {
                    linear: start.clone(),
                    physical: physical_address(&first_entry, start),
                    denied,
                    unavailable,
                    watched,
                    intent,
                    constant_bytes,
                });
            }
        }
        let mut finish_denial = |fault_body: BlockBuilder<'_>,
                                 address: Val<I32>,
                                 present: Val<I1>| {
            match on_fault {
                Some(ref mut on_fault) => {
                    let error_code = match intent {
                        Intent::Write => present
                            .unsigned()
                            .extend::<I32>()
                            .or(intent.base_error_code()),
                        // Presence is the only read/fetch permission.
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
                None => {
                    fault_body.yield_((true, physical_address(&first_entry, start), true, false))
                }
            }
        };
        let (unavailable, physical, denied, watched) = if constant_bytes == Some(1) && faulting {
            // A byte needs only the first page check.
            let physical = body.if_value::<I32>(
                &first_denied,
                |fault_body| finish_denial(fault_body, start.clone(), first_entry.truncate::<I1>()),
                |allowed| allowed.yield_(physical_address(&first_entry, start)),
            )?;
            (
                body.value(false)?,
                physical,
                body.value(false)?,
                first_watched.clone(),
            )
        } else {
            // Resolution exits with backing, denial and write-watch classification.
            // Denial exits the inner block with (address, present) for reporting.
            body.block::<(I1, I32, I1, I1)>(|mut access_body, success| {
                let (address, present) = access_body.block::<(I32, I1)>(|mut checks, fault| {
                    let resolve_pages = |mut pages: BlockBuilder<'_>| {
                        let resolution_word = pages.call::<I32>(
                            self.range_resolver,
                            &[
                                start.into(),
                                bytes.sub(1).into(),
                                (&first_entry).into(),
                                required.into(),
                            ],
                        )?;
                        pages.branch_if(
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
                        pages.branch(
                            &success,
                            (
                                resolution_word.and(SCATTERED).ne(0),
                                physical_address(&resolution_word, start),
                                false,
                                resolution_word.and(watch_mask).ne(0),
                            ),
                        )
                    };
                    match constant_bytes {
                        Some(bytes) if bytes <= PAGE_BYTES => {
                            // Aligned power-of-two operands fit in one page.
                            let may_cross = if bytes.is_power_of_two() {
                                start.and(bytes - 1).ne(0)
                            } else {
                                true.into()
                            };
                            checks.if_(may_cross, |mut candidate| {
                                candidate.if_(crosses_page(start, bytes), resolve_pages)
                            })?;
                            checks.branch_if(
                                &first_denied,
                                &fault,
                                (start, first_entry.truncate::<I1>()),
                            )?;
                            checks.branch(
                                &success,
                                (
                                    false,
                                    physical_address(&first_entry, start),
                                    false,
                                    &first_watched,
                                ),
                            )
                        }
                        _ => {
                            if !faulting {
                                checks.branch_if(
                                    bytes.eq(0),
                                    &fault,
                                    (start, first_entry.truncate::<I1>()),
                                )?;
                            }
                            resolve_pages(checks)
                        }
                    }
                })?;
                finish_denial(access_body, address, present)
            })?
        };
        Ok(Access {
            linear: start.clone(),
            physical,
            denied,
            unavailable,
            watched,
            intent,
            constant_bytes,
        })
    }
}
