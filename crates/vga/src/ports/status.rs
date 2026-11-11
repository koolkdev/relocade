//! Cached Input Status 1 callbacks. Endpoints supply their transfer policy.

use crate::Vga;
use relocade_machine::IoContext;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

pub(super) fn read(
    vga: &Vga,
    device: &Rc<RefCell<Vga>>,
    refresh: impl Fn(&mut Vga) + 'static,
) -> impl Fn(IoContext<'_>) -> u8 + 'static {
    let status = vga.raster.status.clone();
    let attribute_phase = vga.registers.attribute.phase_handle();
    let clock = vga.clock.clone();
    let device = Rc::downgrade(device);
    move |_| {
        if status.observe(clock.now()) {
            refresh_edge(&device, &refresh);
        }
        attribute_phase.reset_to_index();
        status.input_status_1()
    }
}

// Ordinary polls never upgrade the weak device or borrow its other state.
#[cold]
#[inline(never)]
fn refresh_edge(device: &Weak<RefCell<Vga>>, refresh: &impl Fn(&mut Vga)) {
    refresh(&mut device.upgrade().expect("connected VGA").borrow_mut());
}
