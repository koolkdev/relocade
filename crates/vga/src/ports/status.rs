//! Cached Input Status 1 reads with raster work only when its deadline is due.

use crate::Vga;
use relocade_machine::IoContext;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

pub(super) fn read(vga: &Vga, device: &Rc<RefCell<Vga>>) -> impl Fn(IoContext<'_>) -> u8 + 'static {
    // Web callbacks can outlive their bus until JavaScript collection. The raster
    // owns this clock-bearing state so retained callbacks cannot delay shutdown.
    let status = Rc::downgrade(&vga.raster.status);
    let device = Rc::downgrade(device);
    let read_status = vga.registers.attribute.handle_read(move || {
        let status = status.upgrade().expect("connected VGA raster");
        if status.observe_now() {
            refresh_raster(&device);
        }
        status.input_status_1()
    });
    move |_| read_status()
}

// Ordinary polls never upgrade the weak device or borrow its other state.
#[cold]
#[inline(never)]
fn refresh_raster(device: &Weak<RefCell<Vga>>) {
    device
        .upgrade()
        .expect("connected VGA")
        .borrow_mut()
        .refresh_raster();
}

#[cfg(test)]
mod tests {
    use super::*;
    use relocade_machine::{Clock, ClockSource, Sleep, Timestamp};
    use std::cell::Cell;

    struct ObservedClock(Rc<Cell<bool>>);

    impl ClockSource for ObservedClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_ticks(0)
        }

        fn sleep_until(&self, _: Timestamp) -> Sleep {
            Sleep::new(async {})
        }
    }

    impl Drop for ObservedClock {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn retained_reader_does_not_keep_the_clock_alive() {
        let released = Rc::new(Cell::new(false));
        let clock = Clock::new(ObservedClock(released.clone()));
        let device = Rc::new(RefCell::new(Vga::with_clock(clock)));
        let reader = read(&device.borrow(), &device);
        assert!(!released.get());
        drop(device);
        assert!(released.get());
        drop(reader);
    }
}
