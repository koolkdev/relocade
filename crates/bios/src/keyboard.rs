//! BIOS keystrokes shared by interrupt services and console drivers.

mod data;
mod encoding;
mod layout;
mod physical;
mod scan;
pub use encoding::{KeyMode, KeyStroke};
pub use physical::PhysicalKeyboard;

use data::{DataArea, HeldKeys, KeyboardStatus, ShiftFlags};
use scan::Decoder;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, Memory,
};
use std::{
    cell::RefCell,
    future::Future,
    io::{self, Write},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

/// Modifier and toggle states reported by INT 16h/AH=02h.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShiftState {
    pub right_shift: bool,
    pub left_shift: bool,
    pub control: bool,
    pub alt: bool,
    pub scroll_lock: bool,
    pub num_lock: bool,
    pub caps_lock: bool,
    pub insert: bool,
}

impl ShiftState {
    fn from_bits(bits: u8) -> Self {
        let flags = ShiftFlags::from_bits_retain(bits);
        Self {
            right_shift: flags.contains(ShiftFlags::RIGHT_SHIFT),
            left_shift: flags.contains(ShiftFlags::LEFT_SHIFT),
            control: flags.contains(ShiftFlags::CONTROL),
            alt: flags.contains(ShiftFlags::ALT),
            scroll_lock: flags.contains(ShiftFlags::SCROLL_LOCK),
            num_lock: flags.contains(ShiftFlags::NUM_LOCK),
            caps_lock: flags.contains(ShiftFlags::CAPS_LOCK),
            insert: flags.contains(ShiftFlags::INSERT),
        }
    }

    fn bits(self) -> u8 {
        let mut flags = ShiftFlags::empty();
        flags.set(ShiftFlags::RIGHT_SHIFT, self.right_shift);
        flags.set(ShiftFlags::LEFT_SHIFT, self.left_shift);
        flags.set(ShiftFlags::CONTROL, self.control);
        flags.set(ShiftFlags::ALT, self.alt);
        flags.set(ShiftFlags::SCROLL_LOCK, self.scroll_lock);
        flags.set(ShiftFlags::NUM_LOCK, self.num_lock);
        flags.set(ShiftFlags::CAPS_LOCK, self.caps_lock);
        flags.set(ShiftFlags::INSERT, self.insert);
        flags.bits()
    }

    fn shift_pressed(self) -> bool {
        self.left_shift || self.right_shift
    }

    fn numeric_keypad(self) -> bool {
        self.num_lock ^ self.shift_pressed()
    }
}

struct Input {
    data: DataArea,
    decoder: Decoder,
    waker: Option<Waker>,
}

/// An owned handle to the guest's BIOS keyboard data area.
///
/// Physical input reaches the BDA through the default IRQ1 handler. INT 16h and
/// DOS consume that same guest-visible ring; replacing INT 09h replaces translation.
/// Clones share one pending read registration. The guest RAM ring is the only
/// keystroke queue. `Write` stores character bytes with scan code zero and reports
/// short writes or `WouldBlock` when the ring is full.
#[derive(Clone)]
pub struct Keyboard(Rc<RefCell<Input>>);

impl Keyboard {
    /// Initializes keyboard flags and the buffer in a new session's low-memory RAM.
    pub fn new(memory: Memory) -> Self {
        Self(Rc::new(RefCell::new(Input {
            data: DataArea::new(memory),
            decoder: Decoder::default(),
            waker: None,
        })))
    }

    pub(crate) fn scan_code(&self, byte: u8) {
        let waker = {
            let mut input = self.0.borrow_mut();
            let Input { data, decoder, .. } = &mut *input;
            if decoder.scan(data, byte) {
                input.waker.take()
            } else {
                None
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Stores one translated keystroke in the guest ring, as for INT 16h/AH=05h.
    /// Returns false when full, leaving the buffer unchanged. A successful store
    /// wakes a pending reader. This does not produce scan bytes or hardware IRQs.
    #[must_use]
    #[interrupt(
        args(key = KeyStroke { character: cl, scan_code: ch }),
        returns { true => { al = 0 }, false => { al = 1 } }
    )]
    pub fn store_key(&self, key: KeyStroke) -> bool {
        let waker = {
            let mut input = self.0.borrow_mut();
            if !input.data.push(key) {
                return false;
            }
            input.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }

    /// Returns the modifier and toggle states, as for INT 16h/AH=02h.
    #[interrupt(returns { shifts => { al = shifts.bits() } })]
    pub fn shift_state(&self) -> ShiftState {
        self.0.borrow().data.shift_state()
    }

    #[interrupt(returns { flags => { ax = flags } })]
    fn enhanced_shift_state(&self) -> u16 {
        let flags = self.0.borrow().data.flags();
        let held = flags.held
            & (HeldKeys::LEFT_CONTROL
                | HeldKeys::LEFT_ALT
                | HeldKeys::SCROLL_LOCK
                | HeldKeys::NUM_LOCK
                | HeldKeys::CAPS_LOCK);
        let right = flags.status & (KeyboardStatus::RIGHT_CONTROL | KeyboardStatus::RIGHT_ALT);
        // AH bit 7 means SysRq held, not the Insert-held bit at BDA 0040:0018.
        let sys_request = u8::from(flags.held.contains(HeldKeys::SYS_REQUEST)) << 7;
        u16::from_le_bytes([
            flags.shifts.bits(),
            held.bits() | right.bits() | sys_request,
        ])
    }

    pub fn set_shift_state(&self, shifts: ShiftState) {
        self.0.borrow().data.set_shift_state(shifts);
    }

    /// Discards queued keystrokes without changing modifier states.
    pub fn discard_queued(&self) {
        self.0.borrow().data.discard();
    }

    /// Reports the next keystroke for INT 16h/AH=01h or AH=11h without consuming it.
    /// Legacy status discards incompatible entries preceding that key.
    #[interrupt(returns {
        Some(key) => { al = key.character, ah = key.scan_code, zf = false },
        None => { zf = true },
    })]
    pub fn peek_key(&self, mode: KeyMode) -> Option<KeyStroke> {
        let input = self.0.borrow();
        loop {
            let key = input.data.peek()?;
            if let Some(key) = mode.translate(key) {
                return Some(key);
            }
            input.data.read();
        }
    }

    /// Consumes the next whole keystroke, or returns None without waiting.
    pub fn try_read_key(&self, mode: KeyMode) -> Option<KeyStroke> {
        let key = self.peek_key(mode)?;
        self.0.borrow().data.read();
        Some(key)
    }

    /// Waits for and consumes one keystroke for INT 16h/AH=00h or AH=10h.
    #[interrupt(returns { key => { al = key.character, ah = key.scan_code } })]
    pub async fn read_key(&self, mode: KeyMode) -> KeyStroke {
        loop {
            if let Some(key) = self.try_read_key(mode) {
                return key;
            }
            self.wait_for_key().await;
        }
    }

    fn wait_for_key(&self) -> WaitForKey<'_> {
        WaitForKey { keyboard: self }
    }
}

#[interrupt(context(call), args(function = ah))]
pub(crate) async fn dispatch(call: &mut CpuContext<'_>, keyboard: &Keyboard, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => keyboard.read_key(KeyMode::Legacy),
            0x01 => keyboard.peek_key(KeyMode::Legacy),
            0x02 => keyboard.shift_state,
            0x05 => keyboard.store_key,
            0x10 => keyboard.read_key(KeyMode::Enhanced),
            0x11 => keyboard.peek_key(KeyMode::Enhanced),
            0x12 => keyboard.enhanced_shift_state,
            _ => crate::interrupt::unsupported,
        }
    );
}

impl Write for Keyboard {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for (written, &character) in bytes.iter().enumerate() {
            if !self.store_key(KeyStroke {
                character,
                scan_code: 0,
            }) {
                return if written == 0 {
                    Err(io::ErrorKind::WouldBlock.into())
                } else {
                    Ok(written)
                };
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct WaitForKey<'a> {
    keyboard: &'a Keyboard,
}

impl Future for WaitForKey<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut input = self.keyboard.0.borrow_mut();
        if input.data.peek().is_none() {
            input.waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for WaitForKey<'_> {
    fn drop(&mut self) {
        self.keyboard.0.borrow_mut().waker = None;
    }
}

#[cfg(test)]
mod tests;
