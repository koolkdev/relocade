//! Publish captures without holding a device borrow across a wait.
use super::CardOutput;
use futures_lite::future::or;
use std::{cell::RefCell, rc::Rc};

/// The task maps each taken timestamp to host time before waiting. New captures
/// replace pending work without extending a wait already in progress.
pub(crate) async fn run<Card: CardOutput>(device: Rc<RefCell<Card>>) {
    let (clock, sender) = {
        let mut device = device.borrow_mut();
        let output = device.output();
        (output.clock.clone(), output.sender.clone())
    };
    loop {
        let pending = {
            let mut device = device.borrow_mut();
            device.capture_if_due();
            device.output().pending.take()
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
                let output = device.output();
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
