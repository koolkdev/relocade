//! DOS idle notifications while waiting for input or reporting an empty poll.

use crate::{
    interrupts::{IDLE, IDLE_ENTRY},
    services::control::Request,
};
use relocade_machine::{real_mode_interrupt::read_vector, CpuContext};
use std::{
    future::{poll_fn, Future},
    pin::pin,
    task::Poll,
};

const DIRECTION_FLAG: u32 = 1 << 10;

/// Retains the input future across idle callbacks. The known empty handler lets
/// the read sleep; a replacement vector is observed on every wake or callback.
pub(super) async fn wait_for_input<F: Future>(
    call: &mut CpuContext<'_>,
    request: &Request<'_>,
    input: F,
) -> F::Output {
    let mut input = pin!(input);
    loop {
        let ready = poll_fn(|cx| match input.as_mut().poll(cx) {
            Poll::Ready(value) => Poll::Ready(Some(value)),
            Poll::Pending
                if may_notify(request)
                    && read_vector(&call.memory, IDLE).address() != IDLE_ENTRY.address() =>
            {
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        })
        .await;
        if let Some(value) = ready {
            return value;
        }
        invoke_idle(call).await;
    }
}

/// Empty polls notify once, including the ordinary returning default handler.
pub(super) async fn notify_idle(call: &mut CpuContext<'_>, request: &Request<'_>) {
    if may_notify(request) {
        invoke_idle(call).await;
    }
}

fn may_notify(request: &Request<'_>) -> bool {
    (0x01..=0x0c).contains(&request.function) && request.services.dos.indos().allows_idle()
}

async fn invoke_idle(call: &mut CpuContext<'_>) {
    // DOS runs with CLD; the original INT21 frame preserves the caller's DF.
    call.cpu.set_eflags(call.cpu.eflags() & !DIRECTION_FLAG);
    call.call_interrupt(IDLE).await;
}
