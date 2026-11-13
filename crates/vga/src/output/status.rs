//! Cached status polling, with a card borrow only when raster work is due.
use crate::raster::RasterStatus;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

pub(crate) fn read<Card: 'static>(
    status: &Rc<RasterStatus>,
    device: &Rc<RefCell<Card>>,
    refresh: fn(&mut Card),
) -> impl Fn() -> u8 + 'static {
    // Web callbacks can outlive their bus until JavaScript collection. Only the
    // live raster owns the clock-bearing status; retained readers keep it weak.
    let status = Rc::downgrade(status);
    let device = Rc::downgrade(device);
    move || {
        let status = status.upgrade().expect("connected video raster");
        if status.observe_now() {
            refresh_raster(&device, refresh);
        }
        status.input_status_1()
    }
}

#[cold]
#[inline(never)]
fn refresh_raster<Card>(device: &Weak<RefCell<Card>>, refresh: fn(&mut Card)) {
    refresh(&mut device.upgrade().expect("connected video card").borrow_mut());
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
        let device = Rc::new(RefCell::new(crate::raster::Raster::new(
            clock,
            crate::Timing::vga(&crate::Registers::default()),
        )));
        let reader = read(&device.borrow().status, &device, |raster| {
            raster.update_observed();
        });
        let reader = crate::registers::attribute::Attribute::default().handle_read(reader);
        // A cached poll must also work while the whole raster is borrowed.
        let raster = device.borrow_mut();
        assert_eq!(reader(), raster.status.input_status_1());
        drop(raster);
        assert!(!released.get());
        drop(device);
        assert!(released.get());
        drop(reader);
    }
}
