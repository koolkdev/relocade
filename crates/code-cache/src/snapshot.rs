//! Reserve live fetch dependencies before copying bytes for a worker.

use super::{CodeCache, CodeRange, Ticket};
use std::ops::Range;
use wasm86_x86::CpuState;

/// Protected fetch ranges. Engine adapters copy them synchronously after reserve
/// and before returning to guest execution; no guest memory goes to a worker.
pub struct Capture {
    pub ticket: Ticket,
    pub eip: u32,
    pub instruction_limit: u32,
    spans: Vec<Range<usize>>,
}
impl Capture {
    /// Copies only readable fetch bytes. A short snapshot lets generation reject
    /// an incomplete instruction while accepting an earlier block-ending branch.
    pub fn copy_bytes(&self, backing: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.spans.iter().map(|span| span.len()).sum());
        for span in &self.spans {
            bytes.extend_from_slice(&backing[span.clone()]);
        }
        bytes
    }
}

impl CodeCache {
    /// Reserves a snapshot of at most `15 * instruction_limit` live bytes.
    /// Watches cover pending work before any bytes are copied. Call only at an
    /// execution boundary, and cancel the returned ticket if enqueueing fails.
    pub fn capture(
        &mut self,
        cpu: &CpuState,
        eip: u32,
        instruction_limit: u32,
        table: &mut [u8],
    ) -> Option<Capture> {
        if instruction_limit == 0 {
            return None;
        }
        let bytes = instruction_limit.checked_mul(15)?;
        let cs = self.code_context(cpu, table)?;
        let spans = self.fetch_spans(cs, CodeRange { offset: eip, bytes });
        if spans.is_empty() {
            return None;
        }
        let ticket = self.protect(eip, &spans, table);
        Some(Capture {
            ticket,
            eip,
            instruction_limit,
            spans: spans.into_iter().map(|span| span.bytes).collect(),
        })
    }
}
