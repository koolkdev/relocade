use crate::*;
use relocade_machine::{DeviceConnection, IoContext, PortRead, PortWrite};

pub(super) type Service = dyn Fn(IoContext<'_>);

impl Dma {
    /// Connects a peripheral's elapsed-transfer service to a channel. Register
    /// access services peripherals before observing or changing controller state.
    /// The callback may borrow this controller to transfer bytes, and should
    /// retain its peripheral weakly to avoid a device ownership cycle.
    pub fn attach_channel(&mut self, channel: u8, service: impl Fn(IoContext<'_>) + 'static) {
        self.services[usize::from(channel)] = Some(Rc::new(service));
    }

    pub fn configure_io(io: &mut DeviceConnection<'_>, dma: &Rc<RefCell<Self>>) {
        let unreadable = PortRead::byte(|_| 0xff);
        for channel in 0..4 {
            for (register, port) in [
                (channel * 2, ADDRESS_PORTS[channel]),
                (channel * 2 + 1, COUNT_PORTS[channel]),
            ] {
                io.bind(
                    port,
                    &read(dma, move |dma| dma.read_word(register)),
                    &write(dma, move |dma, value| dma.write_word(register, value)),
                );
            }
            io.bind(
                PAGE_PORTS[channel],
                &read(dma, move |dma| dma.channels[channel].page),
                &write(dma, move |dma, value| dma.channels[channel].page = value),
            );
        }
        io.bind(
            STATUS_PORT,
            &read(dma, |dma| dma.read_status()),
            &write(dma, |dma, value| dma.command = value),
        );
        io.bind(
            REQUEST_PORT,
            &unreadable,
            &write(dma, |dma, value| {
                set_bit(&mut dma.software_requests, value & 3, value & 4 != 0);
            }),
        );
        io.bind(
            SINGLE_MASK_PORT,
            &unreadable,
            &write(dma, |dma, value| {
                set_bit(&mut dma.mask, value & 3, value & 4 != 0);
            }),
        );
        io.bind(
            MODE_PORT,
            &unreadable,
            &write(dma, |dma, value| {
                dma.channels[usize::from(value & 3)].mode = value & 0xfc;
            }),
        );
        io.bind(
            CLEAR_FLIP_FLOP_PORT,
            &unreadable,
            &write(dma, |dma, _| dma.high_byte = false),
        );
        io.bind(
            MASTER_CLEAR_PORT,
            &PortRead::byte(|_| 0), // No memory-to-memory transfers use the temporary register.
            &write(dma, |dma, _| dma.master_clear()),
        );
        io.bind(
            CLEAR_MASK_PORT,
            &unreadable,
            &write(dma, |dma, _| dma.mask = 0),
        );
        io.bind(
            MASK_PORT,
            &unreadable,
            &write(dma, |dma, value| dma.mask = value & 0x0f),
        );
    }
}

fn synchronize(dma: &Rc<RefCell<Dma>>, mut context: IoContext<'_>) {
    // Release the controller borrow before peripherals transfer through it.
    let services = dma.borrow().services.clone();
    for service in services.into_iter().flatten() {
        service(context.reborrow());
    }
}

fn read(dma: &Rc<RefCell<Dma>>, read: impl Fn(&mut Dma) -> u8 + 'static) -> PortRead {
    let dma = Rc::downgrade(dma);
    PortRead::byte(move |context| {
        let dma = dma.upgrade().expect("connected DMA controller");
        synchronize(&dma, context);
        let value = read(&mut dma.borrow_mut());
        value
    })
}

fn write(dma: &Rc<RefCell<Dma>>, write: impl Fn(&mut Dma, u8) + 'static) -> PortWrite {
    let dma = Rc::downgrade(dma);
    PortWrite::byte(move |context, value| {
        let dma = dma.upgrade().expect("connected DMA controller");
        synchronize(&dma, context);
        write(&mut dma.borrow_mut(), value);
    })
}
