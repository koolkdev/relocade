//! Guest retrace capture and host-timed publication on the card's execution owner.
use super::CardOutput;
use futures_lite::future::or;
use std::{cell::RefCell, rc::Rc};

pub(crate) async fn run<Card: CardOutput>(device: Rc<RefCell<Card>>) {
    or(capture_retraces(&device), publish_frames(&device)).await;
}

async fn capture_retraces<Card: CardOutput>(device: &RefCell<Card>) {
    loop {
        let retrace = {
            let mut device = device.borrow_mut();
            device.capture_if_due();
            let output = device.output();
            output.retrace_wait(output.raster.next_retrace_at())
        };
        retrace.await;
    }
}

async fn publish_frames<Card: CardOutput>(device: &RefCell<Card>) {
    let (clock, sender) = {
        let mut device = device.borrow_mut();
        let output = device.output();
        (output.clock.clone(), output.sender.clone())
    };
    loop {
        let pending = device.borrow_mut().output().pending.take();
        if let Some(pending) = pending {
            clock.sync_to_host(pending.time).await;
            if sender.force_send(pending.snapshot).is_err() {
                return;
            }
        } else {
            let captured = device.borrow_mut().output().ready.listen();
            captured.await;
        }
    }
}
