//! Keyboard state shared by BIOS keystrokes and DOS character reads.

mod input;

use relocade_machine::{real_mode_interrupt, CpuContext, HookResult};
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

impl KeyStroke {
    fn word(self) -> u16 {
        u16::from_le_bytes([self.character, self.scan_code])
    }
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
    // DOS returns a function key in two character reads. BIOS already consumed
    // that keystroke when DOS took its zero prefix.
    pending_scan_code: Option<u8>,
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

    pub fn set_shift_state(&self, shifts: ShiftState) {
        self.0.borrow_mut().shifts = shifts;
    }

    /// Discards queued keys and the remainder of a DOS extended-key read.
    /// Modifier states are independent of queued input.
    pub fn discard_queued(&self) {
        let mut input = self.0.borrow_mut();
        input.keys.clear();
        input.pending_scan_code = None;
    }

    /// Reports whether a DOS character read can complete immediately.
    pub fn has_input(&self) -> bool {
        let input = self.0.borrow();
        input.pending_scan_code.is_some() || !input.keys.is_empty()
    }

    /// Takes one DOS character, splitting function keys into zero and scan code.
    pub fn try_read_character(&self) -> Option<u8> {
        let mut input = self.0.borrow_mut();
        if let Some(scan_code) = input.pending_scan_code.take() {
            return Some(scan_code);
        }
        let key = input.keys.pop_front()?;
        if key.character == 0 && key.scan_code != 0 {
            input.pending_scan_code = Some(key.scan_code);
        }
        Some(key.character)
    }

    /// Waits for a DOS character without polling the host or entering guest code.
    pub async fn read_character(&self) -> u8 {
        loop {
            if let Some(character) = self.try_read_character() {
                return character;
            }
            self.wait_for_key().await;
        }
    }

    fn peek(&self) -> Option<KeyStroke> {
        self.0.borrow().keys.front().copied()
    }

    fn try_read(&self) -> Option<KeyStroke> {
        self.0.borrow_mut().keys.pop_front()
    }

    async fn read(&self) -> KeyStroke {
        loop {
            if let Some(key) = self.try_read() {
                return key;
            }
            self.wait_for_key().await;
        }
    }

    fn wait_for_key(&self) -> WaitForKey<'_> {
        WaitForKey { keyboard: self }
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

pub(super) async fn interrupt<T>(call: &mut CpuContext<'_, T>, keyboard: &Keyboard) -> HookResult {
    match call.cpu.ah() {
        0x00 => {
            let key = keyboard.read().await;
            call.cpu.set_ax(key.word());
        }
        0x01 => {
            let key = keyboard.peek();
            if let Some(key) = key {
                call.cpu.set_ax(key.word());
            }
            real_mode_interrupt::set_zero(call, key.is_none());
        }
        0x02 => call.cpu.set_al(keyboard.0.borrow().shifts.bits()),
        _ => {
            call.cpu.set_ah(0x86);
            real_mode_interrupt::set_carry(call, true);
        }
    }
    real_mode_interrupt::return_from_interrupt(call)
}
