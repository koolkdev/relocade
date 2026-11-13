//! Publish captures without holding a device borrow across a wait.
use super::Output;
use futures_lite::future::or;
use std::{cell::RefCell, rc::Rc};

/// The task maps each taken timestamp to host time before waiting. New captures
/// replace pending work without extending a wait already in progress.
pub(crate) async fn run<Card>(
    device: Rc<RefCell<Card>>,
    output: fn(&mut Card) -> &mut Output,
    capture_if_due: fn(&mut Card),
) {
    let (clock, sender) = {
        let mut device = device.borrow_mut();
        let output = output(&mut device);
        (output.clock.clone(), output.sender.clone())
    };
    loop {
        let pending = {
            let mut device = device.borrow_mut();
            capture_if_due(&mut device);
            output(&mut device).pending.take()
        };
        if let Some(pending) = pending {
            clock.sync_to_host(pending.time).await;
            if sender.force_send(pending.snapshot).is_err() {
                return;
            }
        } else {
            // All producers are on this owner. No await separates the pending
            // check from installing both wake paths, and no borrow spans a wait.
            let wait = {
                let mut device = device.borrow_mut();
                let output = output(&mut device);
                let captured = output.ready.listen();
                let retrace = output.retrace_wait(output.raster.next_retrace_at());
                async move {
                    or(captured, async {
                        retrace.await;
                    })
                    .await;
                }
            };
            wait.await;
        }
    }
}
