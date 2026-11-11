//! Cached Input Status 1 readers and the colocated Feature Control writes.

use crate::Vga;
use relocade_machine::{PortRead, PortWrite};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

pub(super) fn read(
    vga: &Vga,
    device: &Rc<RefCell<Vga>>,
    refresh: impl Fn(&mut Vga) + 'static,
) -> PortRead {
    let status = vga.raster.status.clone();
    let data_next = vga.registers.attribute.data_next.clone();
    let clock = vga.clock.clone();
    let device = Rc::downgrade(device);
    PortRead::new(move |_, _, _| {
        if status.observe(clock.now()) {
            refresh_edge(&device, &refresh);
        }
        data_next.set(false);
        // Neither alias has implemented trailing bytes. Callers truncate the carrier.
        u32::from(status.input_status_1()) | 0xffff_ff00
    })
}

pub(super) fn write(device: &Rc<RefCell<Vga>>) -> PortWrite {
    let device = Rc::downgrade(device);
    PortWrite::new(move |_, _, _, value| {
        device
            .upgrade()
            .expect("connected VGA")
            .borrow_mut()
            .registers
            .feature_control = value as u8;
    })
}

// Ordinary polls never upgrade the weak device or borrow its other state.
#[cold]
#[inline(never)]
fn refresh_edge(device: &Weak<RefCell<Vga>>, refresh: &impl Fn(&mut Vga)) {
    refresh(&mut device.upgrade().expect("connected VGA").borrow_mut());
}
