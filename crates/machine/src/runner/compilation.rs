//! Automatic function requests and installation on one execution owner.

use crate::{Clock, CompilerClient, Cpu, FarPointer, FunctionRequest};
use relocade_x86::FunctionLimits;
use std::{
    collections::VecDeque,
    future::Future,
    pin::pin,
    task::{Context, Poll},
};

const MAX_PENDING: usize = 8;

pub(super) struct Compilation {
    client: CompilerClient,
    pending: VecDeque<FarPointer>,
    clock: Clock,
}

impl Compilation {
    pub(super) fn new(client: CompilerClient, clock: Clock) -> Self {
        Self {
            client,
            pending: VecDeque::new(),
            clock,
        }
    }

    pub(super) fn into_client(self) -> CompilerClient {
        assert!(
            self.pending.is_empty(),
            "take the compiler before automatic requests begin"
        );
        self.client
    }

    /// Submits newly hot candidates and polls ordered replies between guest turns.
    /// Returns false when the optional connection can no longer be used.
    pub(super) fn poll(&mut self, cpu: &mut Cpu, cx: &mut Context<'_>) -> bool {
        cpu.poll_profiling(&self.clock, cx);
        while self.pending.len() < MAX_PENDING {
            let Some(entry) = cpu.next_hot_function() else {
                break;
            };
            if cpu.has_entry(entry.address()) {
                continue;
            }
            let request = FunctionRequest {
                entry,
                limits: FunctionLimits::default(),
            };
            if self
                .client
                .request_function(cpu.cpu_state().execution_target(), request)
                .is_err()
            {
                return false;
            }
            self.pending.push_back(entry);
        }
        // Pending or parked host services may retain dispatch entries. Keep
        // replies queued until installation can use the ordinary CPU API.
        if !cpu.can_install_code() {
            return true;
        }
        while let Some(&entry) = self.pending.front() {
            let reply = match pin!(self.client.recv()).poll(cx) {
                Poll::Pending => break,
                Poll::Ready(Some(reply)) => reply,
                Poll::Ready(None) => return false,
            };
            self.pending.pop_front();
            if cpu.has_entry(entry.address()) {
                continue;
            }
            if let Ok(code) = reply {
                if code
                    .blocks()
                    .iter()
                    .any(|block| block.start_eip == u32::from(entry.offset))
                    && code.install(cpu).is_ok()
                {
                    continue;
                }
            }
            // Rejection is a resolved interpreter entry, so future calls skip
            // recording as well as repeated compilation. No stub module is needed.
            if cpu.interpret_at(entry.address()).is_err() {
                return false;
            }
        }
        true
    }
}
