//! INT 33h register adapters; state changes are synchronous except BIOS reset.

use super::{
    state::{Callback, Counter, Point},
    Mouse,
};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, MemoryView,
};

impl Mouse {
    #[interrupt(context(call), args(function = ax))]
    pub(super) async fn dispatch(&self, call: &mut CpuContext<'_>, function: u16) {
        interrupt_dispatch!(
            call,
            match function {
                0x0000 => self.reset,
                0x0003 => self.position,
                0x0004 => self.set_position,
                0x0005 => self.button_counter(true),
                0x0006 => self.button_counter(false),
                0x0007 => self.set_bounds(true),
                0x0008 => self.set_bounds(false),
                0x000b => self.motion,
                0x000c => self.set_callback,
                0x000f => self.set_ratio,
                0x0013 => self.set_threshold,
                0x0014 => self.exchange_callback,
                0x001a => self.set_sensitivity,
                0x001b => self.sensitivity,
                _ => unsupported,
            }
        );
    }

    #[interrupt(context(call), returns { installed => {
        ax = if installed { 0xffff } else { 0 }, bx = if installed { 3 } else { 0 }
    } })]
    async fn reset(&self, call: &mut CpuContext<'_>) -> bool {
        self.initialize(call).await
    }

    #[interrupt(context(memory), returns { (buttons, point) => { bx = buttons, cx = point.x, dx = point.y } })]
    fn position(&self, memory: &MemoryView<'_>) -> (u16, Point) {
        let state = self.state.borrow();
        (state.buttons, state.position(memory))
    }

    #[interrupt(args(x = cx, y = dx))]
    fn set_position(&self, x: u16, y: u16) {
        let mut state = self.state.borrow_mut();
        state.x.set_position(i32::from(x as i16));
        state.y.set_position(i32::from(y as i16));
    }

    #[interrupt(args(button = bx), returns { (buttons, counter) => {
        ax = buttons, bx = counter.count, cx = counter.position.x, dx = counter.position.y
    } })]
    fn button_counter(&self, pressed: bool, button: u16) -> (u16, Counter) {
        let mut state = self.state.borrow_mut();
        (state.buttons, state.take_counter(button, pressed))
    }

    #[interrupt(args(first = cx, last = dx))]
    fn set_bounds(&self, horizontal: bool, first: u16, last: u16) {
        let mut state = self.state.borrow_mut();
        let axis = if horizontal {
            &mut state.x
        } else {
            &mut state.y
        };
        axis.set_bounds(first as i16, last as i16);
    }

    #[interrupt(returns { [x, y] => { cx = x as u16, dx = y as u16 } })]
    fn motion(&self) -> [i16; 2] {
        std::mem::take(&mut self.state.borrow_mut().motion)
    }

    #[interrupt(args(mask = cx, target = ptr(es, dx)))]
    fn set_callback(&self, mask: u16, target: FarPointer) {
        self.state.borrow_mut().callback = Callback { mask, target };
    }

    #[interrupt(args(mask = cx, target = ptr(es, dx)), returns {
        previous => { cx = previous.mask, ptr(es, dx) = previous.target }
    })]
    fn exchange_callback(&self, mask: u16, target: FarPointer) -> Callback {
        std::mem::replace(
            &mut self.state.borrow_mut().callback,
            Callback { mask, target },
        )
    }

    #[interrupt(args(x = cx, y = dx))]
    fn set_ratio(&self, x: u16, y: u16) {
        let mut state = self.state.borrow_mut();
        state.x.set_ratio(x);
        state.y.set_ratio(y);
    }

    #[interrupt(args(threshold = dx))]
    fn set_threshold(&self, threshold: u16) {
        self.state
            .borrow_mut()
            .acceleration
            .set_threshold(threshold);
    }

    #[interrupt(args(x = bx, y = cx, threshold = dx))]
    fn set_sensitivity(&self, x: u16, y: u16, threshold: u16) {
        let mut state = self.state.borrow_mut();
        state.x.set_sensitivity(x);
        state.y.set_sensitivity(y);
        state.acceleration.set_threshold(threshold);
    }

    #[interrupt(returns { (x, y, threshold) => { bx = x, cx = y, dx = threshold } })]
    fn sensitivity(&self) -> (u16, u16, u16) {
        let state = self.state.borrow();
        (
            state.x.sensitivity,
            state.y.sensitivity,
            state.acceleration.threshold,
        )
    }
}

#[interrupt]
fn unsupported() {}
