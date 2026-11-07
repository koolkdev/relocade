use futures_channel::oneshot;
use futures_util::future::LocalBoxFuture;
use relocade_machine::{Device, DeviceConnection, IoBus};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
};

#[derive(Default)]
struct Status {
    value: u32,
    change_during_read: bool,
    subscribers: Vec<oneshot::Sender<()>>,
}

impl Status {
    fn set(&mut self, value: u32) {
        self.value = value;
        for subscriber in self.subscribers.drain(..) {
            let _ = subscriber.send(());
        }
    }
}

impl Device for Status {
    fn read_port(&mut self, _: u16, _: u8) -> u32 {
        let value = self.value;
        if std::mem::take(&mut self.change_during_read) {
            self.set(0x1234);
        }
        value
    }

    fn port_changed(&mut self, _: u16) -> LocalBoxFuture<'static, ()> {
        let (notify, changed) = oneshot::channel();
        self.subscribers.push(notify);
        Box::pin(async {
            let _ = changed.await;
        })
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn status_waits_keep_notifications_before_await_and_release_device_borrows() {
    let device = Rc::new(RefCell::new(Status::default()));
    let io = IoBus::new([DeviceConnection {
        device: device.clone(),
        mmio: vec![],
        ports: std::iter::once(0x80..=0x80).collect(),
    }]);
    let mut cx = Context::from_waker(Waker::noop());
    let mut wait = pin!(io.wait_port(0x80, 1, |value| value == 0x34));
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    device.borrow_mut().set(1); // A notification need not satisfy the predicate.
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    device.borrow_mut().change_during_read = true;
    device.borrow_mut().set(2);
    // The first read returns 2 and publishes 1234h before its waiter is polled.
    // The retained notification causes another ordinary, byte-wide bus read.
    assert_eq!(wait.as_mut().poll(&mut cx), std::task::Poll::Ready(0x34));
    assert!(device
        .borrow()
        .subscribers
        .iter()
        .all(oneshot::Sender::is_canceled));
}
