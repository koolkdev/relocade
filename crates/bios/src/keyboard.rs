//! BIOS keystrokes shared by interrupt services and console drivers.

mod input;

use relocade_machine::real_mode_interrupt::{interrupt, interrupt_dispatch};
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    io::{self, Write},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

/// One translated PC keystroke. The character uses the guest's encoding.
/// A zero character with a nonzero scan code represents a function or cursor key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyStroke {
    pub character: u8,
    pub scan_code: u8,
}

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
    fn bits(self) -> u8 {
        u8::from(self.right_shift)
            | (u8::from(self.left_shift) << 1)
            | (u8::from(self.control) << 2)
            | (u8::from(self.alt) << 3)
            | (u8::from(self.scroll_lock) << 4)
            | (u8::from(self.num_lock) << 5)
            | (u8::from(self.caps_lock) << 6)
            | (u8::from(self.insert) << 7)
    }
}

#[derive(Default)]
struct Input {
    keys: VecDeque<KeyStroke>,
    shifts: ShiftState,
    waker: Option<Waker>,
}

/// An owned input handle usable while guest execution awaits a key.
///
/// Local callers can inject translated keys through [`Self::push`] and update
/// modifiers with [`Self::set_shift_state`]. [`Self::connect`] accepts keyboard
/// events across threads and translates them on execution. Clones share one queue.
/// There may be only one pending guest read, whether it is a BIOS or DOS read.
///
/// `std::io::Write` injects character bytes with scan code zero, preserving the
/// existing DOS stdin interface. Use `push` when BIOS scan codes are required.
#[derive(Clone, Default)]
pub struct Keyboard(Rc<RefCell<Input>>);

impl Keyboard {
    /// Registers keyboard input translation as a device activity on the execution
    /// owner. Add the returned connection to the CPU's bus, even though this
    /// BIOS-level adapter does not yet expose keyboard-controller I/O ports.
    /// It accepts ASCII text and base navigation/function scan codes. AltGr and
    /// the enhanced modifier-specific navigation/function codes are not modeled.
    pub fn connect<C: relocade_machine::Clock>(
        &self,
        runner: &relocade_machine::Runner<C>,
    ) -> (
        relocade_frontend::keyboard::KeyboardInput,
        relocade_machine::DeviceConnection,
    ) {
        input::connect(self.clone(), runner)
    }

    /// Queues a translated keystroke and wakes a pending guest read.
    pub fn push(&self, key: KeyStroke) {
        let waker = {
            let mut input = self.0.borrow_mut();
            input.keys.push_back(key);
            input.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Returns the modifier and toggle states, as for INT 16h/AH=02h.
    #[interrupt(returns { shifts => { al = shifts.bits() } })]
    pub fn shift_state(&self) -> ShiftState {
        self.0.borrow().shifts
    }

    pub fn set_shift_state(&self, shifts: ShiftState) {
        self.0.borrow_mut().shifts = shifts;
    }

    /// Discards queued keystrokes without changing modifier states.
    pub fn discard_queued(&self) {
        self.0.borrow_mut().keys.clear();
    }

    /// Reports the next whole keystroke without consuming it, as for INT 16h/AH=01h.
    // IBM's STI / RETF 2 status return leaves IF enabled along with the result ZF.
    #[interrupt(returns {
        Some(key) => { al = key.character, ah = key.scan_code, zf = false, interrupts_enabled = true },
        None => { zf = true, interrupts_enabled = true },
    })]
    pub fn peek_key(&self) -> Option<KeyStroke> {
        self.0.borrow().keys.front().copied()
    }

    /// Consumes the next whole keystroke, or returns None without waiting.
    pub fn try_read_key(&self) -> Option<KeyStroke> {
        self.0.borrow_mut().keys.pop_front()
    }

    /// Waits for and consumes one whole keystroke, as for INT 16h/AH=00h.
    #[interrupt(returns { key => { al = key.character, ah = key.scan_code } })]
    pub async fn read_key(&self) -> KeyStroke {
        loop {
            if let Some(key) = self.try_read_key() {
                return key;
            }
            self.wait_for_key().await;
        }
    }

    fn wait_for_key(&self) -> WaitForKey<'_> {
        WaitForKey { keyboard: self }
    }
}

interrupt_dispatch! {
    pub(crate) dispatch(ah, keyboard: &Keyboard) {
        0x00: keyboard.read_key,
        0x01: keyboard.peek_key,
        0x02: keyboard.shift_state,
        _: crate::interrupt::unsupported,
    }
}

impl Write for Keyboard {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for &character in bytes {
            self.push(KeyStroke {
                character,
                scan_code: 0,
            });
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
        if input.keys.is_empty() {
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
