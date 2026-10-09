//! Resolve code dependencies and protect their complete lifetime.

use super::{CodeCache, Entry, Ticket};
use std::{collections::HashSet, ops::Range};
use wasm86_x86::{CpuState, SegmentKind, StoredSegment};

/// Guest instruction bytes relative to the current CS base. A range can wrap
/// at 32 bits when CS permits it. Registration requires every byte to be mapped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeRange {
    pub offset: u32,
    pub bytes: u32,
}

pub(super) struct CodeSpan {
    page: u32,
    backing: u32,
    pub bytes: Range<usize>,
}

impl CodeCache {
    /// Protects all supplied code ranges without reading or decoding guest bytes.
    /// The ranges must be nonempty and include the entry EIP. Failure leaves
    /// registrations in the same CS context unchanged, including replacements.
    ///
    /// The loader is responsible for matching an artifact to the guest image and
    /// this owner's profile/context. Keep its ticket protected through any async
    /// loading/compilation, and cancel it if the artifact cannot be installed.
    pub fn register(
        &mut self,
        cpu: &CpuState,
        eip: u32,
        ranges: &[CodeRange],
        table: &mut [u8],
    ) -> Option<Ticket> {
        if ranges.iter().any(|range| range.bytes == 0)
            || !ranges
                .iter()
                .any(|range| eip.wrapping_sub(range.offset) < range.bytes)
        {
            return None;
        }
        let cs = self.code_context(cpu, table)?;
        let mut spans = Vec::new();
        for &range in ranges {
            let fetched = self.fetch_spans(cs, range);
            let bytes: u64 = fetched.iter().map(|span| span.bytes.len() as u64).sum();
            if bytes != u64::from(range.bytes) {
                return None;
            }
            spans.extend(fetched);
        }
        Some(self.protect(eip, &spans, table))
    }

    pub(super) fn code_context(
        &mut self,
        cpu: &CpuState,
        table: &mut [u8],
    ) -> Option<StoredSegment> {
        let cs = cpu.segments.cs;
        (self.enter(cpu, table) && matches!(cs.attributes.kind(), Some(SegmentKind::Code { .. })))
            .then_some(cs)
    }

    /// The mapped prefix of one range. Snapshots can compile a short prefix;
    /// registration of an existing artifact requires the complete range.
    pub(super) fn fetch_spans(&self, cs: StoredSegment, range: CodeRange) -> Vec<CodeSpan> {
        let mut remaining = range.bytes;
        let mut offset = range.offset;
        let mut spans = Vec::new();
        while remaining != 0 && offset <= cs.limit {
            let linear = cs.base.wrapping_add(offset);
            let Some(backing) = self.mappings.get(linear >> 12).backing() else {
                break;
            };
            let mut count = remaining.min(4096 - (linear & 4095));
            if cs.limit != u32::MAX {
                count = count.min(cs.limit - offset + 1);
            }
            let start = (backing + (linear & 4095)) as usize;
            spans.push(CodeSpan {
                page: linear >> 12,
                backing,
                bytes: start..start + count as usize,
            });
            remaining -= count;
            offset = offset.wrapping_add(count);
            if cs.limit != u32::MAX && offset == 0 {
                break;
            }
        }
        spans
    }

    pub(super) fn protect(&mut self, eip: u32, spans: &[CodeSpan], table: &mut [u8]) -> Ticket {
        let mappings: HashSet<_> = spans.iter().map(|span| span.page).collect();
        let backing: HashSet<_> = spans.iter().map(|span| span.backing).collect();
        if let Some(previous) = self.pending.get(&eip).copied() {
            self.cancel(previous, table);
        }
        let ticket = Ticket(self.next_ticket);
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .expect("code ticket overflow");
        for &page in &mappings {
            self.by_mapping.entry(page).or_default().insert(ticket);
        }
        for &frame in &backing {
            let tickets = self.by_backing.entry(frame).or_default();
            let first = tickets.is_empty();
            tickets.insert(ticket);
            if first {
                self.mappings.watch_aliases(table, frame, true);
            }
        }
        self.entries.insert(
            ticket,
            Entry {
                eip,
                mappings,
                backing,
            },
        );
        self.pending.insert(eip, ticket);
        ticket
    }
}
