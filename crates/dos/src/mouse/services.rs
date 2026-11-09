//! INT 33h register adapters; state changes are synchronous except BIOS reset.

use super::{
    snapshot,
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
                0x0001 => self.visibility(true),
                0x0002 => self.visibility(false),
                0x0003 => self.position,
                0x0004 => self.set_position,
                0x0005 => self.button_counter(true),
                0x0006 => self.button_counter(false),
                0x0007 => self.set_bounds(true),
                0x0008 => self.set_bounds(false),
                0x0009 => self.set_graphics_cursor,
                0x000a => self.set_text_cursor,
                0x000b => self.motion,
                0x000c => self.set_callback,
                0x000f => self.set_ratio,
                0x0013 => self.set_threshold,
                0x0014 => self.exchange_callback,
                0x0015 => self.storage_size,
                0x0016 => self.save_state,
                0x0017 => self.restore_state,
                0x001a => self.set_sensitivity,
                0x001b => self.sensitivity,
                0x0024 => self.version,
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

    #[interrupt(returns { (buttons, point) => { bx = buttons, cx = point.x, dx = point.y } })]
    fn position(&self) -> (u16, Point) {
        let coordinates = self.coordinates();
        let state = self.state.borrow();
        (state.buttons, state.position(coordinates))
    }

    #[interrupt(args(x = cx, y = dx))]
    fn set_position(&self, x: u16, y: u16) {
        {
            let mut state = self.state.borrow_mut();
            state.x.set_position(i32::from(x as i16));
            state.y.set_position(i32::from(y as i16));
        }
        self.update_cursor();
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
        {
            let mut state = self.state.borrow_mut();
            let axis = if horizontal {
                &mut state.x
            } else {
                &mut state.y
            };
            axis.set_bounds(first as i16, last as i16);
        }
        self.update_cursor();
    }

    #[interrupt]
    fn visibility(&self, show: bool) {
        self.cursor.borrow_mut().visibility(show);
        self.update_cursor();
    }

    #[interrupt(args(kind = bx, screen = cx, cursor = dx))]
    fn set_text_cursor(&self, kind: u16, screen: u16, cursor: u16) {
        if kind == 0 {
            self.cursor.borrow_mut().set_text_mask(screen, cursor);
            self.update_cursor();
        }
    }

    #[interrupt(context(memory), args(x = bx, y = cx, source = ptr(es, dx)))]
    fn set_graphics_cursor(&self, memory: &MemoryView<'_>, x: u16, y: u16, source: FarPointer) {
        self.cursor
            .borrow_mut()
            .set_graphics_mask(memory, source, [x as i16, y as i16]);
        self.update_cursor();
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

    #[interrupt(returns { size => { bx = size } })]
    fn storage_size(&self) -> u16 {
        snapshot::BYTE_LEN as u16
    }

    #[interrupt(context(memory), args(destination = ptr(es, dx)))]
    fn save_state(&self, memory: &mut MemoryView<'_>, destination: FarPointer) {
        let bytes = snapshot::encode(&self.state.borrow(), &self.cursor.borrow());
        memory
            .write_wrapping(destination, &bytes)
            .expect("mouse state buffer");
    }

    #[interrupt(context(memory), args(source = ptr(es, dx)))]
    fn restore_state(&self, memory: &MemoryView<'_>, source: FarPointer) {
        let mut bytes = [0; snapshot::BYTE_LEN];
        memory
            .read_wrapping(source, &mut bytes)
            .expect("mouse state buffer");
        let Some((state, cursor)) = snapshot::decode(&bytes) else {
            return;
        };
        {
            let mut video = self.video.borrow_mut();
            self.cursor.borrow_mut().reset(&mut video.framebuffer());
        }
        *self.state.borrow_mut() = state;
        *self.cursor.borrow_mut() = cursor;
        self.update_cursor();
    }

    #[interrupt(returns { (version, interface) => { bx = version, cx = interface } })]
    fn version(&self) -> (u16, u16) {
        // Relocade driver 1.0; PS/2 uses type 4 and the conventional IRQ field 0.
        (0x0100, 0x0400)
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
