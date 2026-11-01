use relocade_machine::Device;
use std::task::{Context, Poll, Waker};

#[derive(Default)]
pub struct Irq {
    pending: bool,
    pub acknowledged: usize,
    waker: Option<Waker>,
}

impl Irq {
    pub fn raise(&mut self) {
        self.pending = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Device for Irq {
    fn write_port(&mut self, _: u16, _: u8, _: u32) {
        self.raise();
    }

    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        if std::mem::take(&mut self.pending) {
            self.acknowledged += 1;
            Poll::Ready(0x60)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
