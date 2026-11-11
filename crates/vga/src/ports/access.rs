//! Transfer widths around concrete port operations. Byte accesses call their
//! bound operation directly; wide VGA transfers use the same bus for trailing
//! bytes, bounded to this device's register cluster.

use crate::Vga;
use relocade_machine::{IoContext, PortRead, PortWrite};
use std::{cell::RefCell, rc::Rc};

pub(super) fn byte_read(
    device: &Rc<RefCell<Vga>>,
    last_port: u16,
    read: impl Fn(&mut Vga) -> u8 + 'static,
) -> PortRead {
    let device = Rc::downgrade(device);
    PortRead::new(move |mut context, port, bytes| {
        let first = read(&mut device.upgrade().expect("connected VGA").borrow_mut());
        let mut value = u32::from(first);
        for index in 1..bytes {
            let address = port.wrapping_add(u16::from(index));
            let byte = if address <= last_port {
                context.read_port(address, 1)
            } else {
                0xff
            };
            value |= byte << (index * 8);
        }
        value
    })
}

pub(super) fn byte_write(
    device: &Rc<RefCell<Vga>>,
    last_port: u16,
    write: impl Fn(&mut Vga, IoContext<'_>, u8) + 'static,
) -> PortWrite {
    let device = Rc::downgrade(device);
    PortWrite::new(move |mut context, port, bytes, value| {
        write(
            &mut device.upgrade().expect("connected VGA").borrow_mut(),
            context.reborrow(),
            value as u8,
        );
        // The first operation's borrow has ended. Earlier bytes can replace the
        // handler used by a later byte, without reborrowing an active device.
        for index in 1..bytes {
            let address = port.wrapping_add(u16::from(index));
            if address <= last_port {
                context.write_port(address, 1, value >> (index * 8));
            }
        }
    })
}

pub(super) fn word_read(
    device: &Rc<RefCell<Vga>>,
    read: impl Fn(&mut Vga) -> u16 + 'static,
) -> PortRead {
    let device = Rc::downgrade(device);
    PortRead::new(move |_, _, bytes| {
        if bytes == 2 {
            read(&mut device.upgrade().expect("connected VGA").borrow_mut()).into()
        } else {
            u32::MAX
        }
    })
}

pub(super) fn word_write(
    device: &Rc<RefCell<Vga>>,
    write: impl Fn(&mut Vga, IoContext<'_>, u16) + 'static,
) -> PortWrite {
    let device = Rc::downgrade(device);
    PortWrite::new(move |context, _, bytes, value| {
        if bytes == 2 {
            write(
                &mut device.upgrade().expect("connected VGA").borrow_mut(),
                context,
                value as u16,
            );
        }
    })
}
