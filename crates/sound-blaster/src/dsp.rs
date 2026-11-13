mod playback;
mod protocol;

use crate::{DMA_CHANNEL, IRQ, READ_DATA_PORT, READ_STATUS_PORT, RESET_PORT, WRITE_DATA_PORT};
use event_listener::Event;
use futures_lite::future::or;
use playback::Playback;
use protocol::{Command, Protocol};
use relocade_dma::Dma;
use relocade_machine::{
    Clock, Device, DeviceCallbacks, DeviceConnection, IoBus, IoContext, Memory, PortRead,
    PortWrite, WeakIoBus,
};
use relocade_pic::Pic;
use std::{cell::RefCell, rc::Rc};

pub(super) struct Dsp {
    clock: Clock,
    io: WeakIoBus,
    protocol: Protocol,
    playback: Playback,
    enabled: bool,
    changed: Event,
}

impl Dsp {
    pub fn new(clock: Clock, memory: Memory, io: &IoBus, dma: Rc<RefCell<Dma>>, pic: &Pic) -> Self {
        Self {
            playback: Playback::new(clock.clone(), memory, dma, pic.irq(IRQ, false)),
            clock,
            io: io.downgrade(),
            protocol: Protocol::default(),
            enabled: false,
            changed: Event::new(),
        }
    }

    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let weak = Rc::downgrade(device);
        device
            .borrow()
            .playback
            .dma
            .borrow_mut()
            .attach_channel(DMA_CHANNEL, move |context| {
                if let Some(device) = weak.upgrade() {
                    device.borrow_mut().advance(context);
                }
            });
        let callbacks = DeviceCallbacks::new(device);
        io.bind(
            RESET_PORT,
            &PortRead::byte(|_| 0xff),
            &PortWrite::byte(callbacks.write(|context, dsp, value| dsp.reset(context, value))),
        );
        io.bind(
            READ_DATA_PORT,
            &PortRead::byte(callbacks.read(|context, dsp| {
                dsp.advance(context);
                dsp.protocol.read_data(&dsp.clock)
            })),
            &PortWrite::byte(|_, _| {}),
        );
        io.bind(
            WRITE_DATA_PORT,
            &PortRead::byte(callbacks.read(|context, dsp| {
                dsp.advance(context);
                dsp.protocol.write_status(&dsp.clock)
            })),
            &PortWrite::byte(callbacks.write(|context, dsp, value| dsp.write(context, value))),
        );
        io.bind(
            READ_STATUS_PORT,
            &PortRead::byte(callbacks.read(|context, dsp| {
                dsp.advance(context);
                dsp.playback.acknowledge();
                dsp.protocol.read_status(&dsp.clock)
            })),
            &PortWrite::byte(|_, _| {}),
        );
    }

    fn advance(&mut self, context: IoContext<'_>) {
        self.playback.advance(context);
    }

    fn synchronize(&mut self) {
        if self.playback.is_due() {
            let io = self.io.upgrade().expect("connected Sound Blaster bus");
            io.with_context(|context| self.advance(context));
        }
    }

    fn reset(&mut self, context: IoContext<'_>, value: u8) {
        self.advance(context);
        self.protocol.reset(&self.clock, value);
        if value & 1 != 0 {
            self.playback.reset();
            self.enabled = false;
            self.changed.notify(1);
        }
    }

    fn write(&mut self, context: IoContext<'_>, value: u8) {
        self.advance(context);
        match self.protocol.write(&self.clock, value) {
            Some(Command::TimeConstant(value)) => self.playback.set_time_constant(value),
            Some(Command::Playback(count)) => self.playback.start(count),
            Some(Command::Speaker(enabled)) => self.enabled = enabled,
            Some(Command::SpeakerStatus) => {
                self.protocol.respond([if self.enabled { 0xff } else { 0 }])
            }
            None => return,
        }
        self.changed.notify(1);
    }
}

impl Device for Dsp {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        loop {
            let (deadline, changed) = {
                let device = device.borrow();
                (device.playback.deadline(), device.changed.listen())
            };
            if let Some(deadline) = deadline {
                or(clock.sleep_until(deadline), changed).await;
            } else {
                changed.await;
            }
            device.borrow_mut().synchronize();
        }
    }
}
