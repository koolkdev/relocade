use relocade_machine::{Device, IoContext};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
};

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
    #[allow(dead_code)]
    pub fn configure_io(
        io: &mut relocade_machine::DeviceConnection<'_>,
        device: &Rc<RefCell<Self>>,
    ) {
        let callbacks = relocade_machine::DeviceCallbacks::new(device);
        io.bind(
            0x80,
            &relocade_machine::PortRead::new(|_, _, _| u32::MAX),
            &relocade_machine::PortWrite::low_byte(callbacks.write(|_, device, _| device.raise())),
        );
    }
}

impl Device for Irq {
    fn poll_interrupt(&mut self, _context: IoContext<'_>, cx: &mut Context<'_>) -> Poll<u8> {
        if std::mem::take(&mut self.pending) {
            self.acknowledged += 1;
            Poll::Ready(0x60)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
