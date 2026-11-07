//! 8042 registers, command handling and keyboard wire translation.

use crate::{
    scancodes, DATA_PORT, DISABLE_INTERFACE, ENABLE_INTERFACE, INPUT_BUFFER_FULL,
    OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT,
};
use event_listener::Event;
use futures_util::future::{select, Either};
use relocade_machine::Clock;
use relocade_pic::{IrqLine, Pic, KEYBOARD_IRQ};
use std::{
    cell::{Cell, RefCell},
    pin::pin,
    time::Duration,
};

// Nominal 12.5 kHz keyboard clock: start, eight data, parity and stop bits.
// A pending byte keeps its deadline across polls of the keyboard activity.
const SERIAL_BIT_TIME: Duration = Duration::from_micros(80);
const SERIAL_FRAME_TIME: Duration = SERIAL_BIT_TIME.saturating_mul(11);

#[derive(Clone, Copy)]
struct Transmission {
    byte: u8,
    deadline: Duration,
}

const IRQ_ENABLED: u8 = 1;
const SYSTEM_INITIALIZED: u8 = 1 << 2;
const COMMAND_INPUT: u8 = 1 << 3;
const KEYBOARD_UNLOCKED: u8 = 1 << 4;
const INTERFACE_DISABLED: u8 = 1 << 4;
const TRANSLATE_SET_2: u8 = 1 << 6;

const READ_COMMAND_BYTE: u8 = 0x20;
const WRITE_COMMAND_BYTE: u8 = 0x60;
const SELF_TEST: u8 = 0xaa;
const INTERFACE_TEST: u8 = 0xab;
const WRITE_OUTPUT_BUFFER: u8 = 0xd2;

#[derive(Clone, Copy)]
enum Input {
    Command(u8),
    Data(u8),
}

struct Registers {
    input: Option<Input>,
    output: u8,
    output_full: bool,
    command_byte: u8,
    last_command: bool,
    break_prefix: bool,
}

pub(super) struct Controller {
    registers: RefCell<Registers>,
    changed: Event,
    irq: IrqLine,
    transmission: Cell<Option<Transmission>>,
    host_transmitting: Cell<bool>,
}

impl Controller {
    pub fn changed(&self) -> event_listener::EventListener {
        self.changed.listen()
    }

    pub fn new(pic: &Pic) -> Self {
        Self {
            registers: RefCell::new(Registers {
                input: None,
                // Deterministic emulator startup value; the initial latch
                // contents are not specified by the hardware.
                output: 0xff,
                output_full: false,
                command_byte: IRQ_ENABLED | SYSTEM_INITIALIZED | TRANSLATE_SET_2,
                last_command: false,
                break_prefix: false,
            }),
            changed: Event::new(),
            irq: pic.irq(KEYBOARD_IRQ, false),
            transmission: Cell::new(None),
            host_transmitting: Cell::new(false),
        }
    }

    pub fn status(&self) -> u8 {
        let registers = self.registers.borrow();
        KEYBOARD_UNLOCKED
            | (registers.command_byte & SYSTEM_INITIALIZED)
            | if registers.last_command {
                COMMAND_INPUT
            } else {
                0
            }
            | if registers.input.is_some() {
                INPUT_BUFFER_FULL
            } else {
                0
            }
            | if registers.output_full {
                OUTPUT_BUFFER_FULL
            } else {
                0
            }
    }

    pub fn read_data(&self) -> u8 {
        let byte = {
            let mut registers = self.registers.borrow_mut();
            // Reading acknowledges the output; the data latch keeps its value
            // until firmware writes another byte, including across IRQ chains.
            registers.output_full = false;
            registers.output
        };
        self.irq.set_level(false);
        self.changed.notify(usize::MAX);
        byte
    }

    pub fn write(&self, port: u16, value: u8) {
        let input = match port {
            DATA_PORT => Input::Data(value),
            STATUS_COMMAND_PORT => Input::Command(value),
            _ => return,
        };
        {
            let mut registers = self.registers.borrow_mut();
            // Software must wait for IBF to clear. A busy write cannot replace
            // the byte already accepted by the controller's input register.
            if registers.input.is_some() {
                return;
            }
            registers.input = Some(input);
            registers.last_command = matches!(input, Input::Command(_));
        }
        self.changed.notify(usize::MAX);
    }

    pub fn inhibited(&self) -> bool {
        let registers = self.registers.borrow();
        registers.output_full
            || registers.command_byte & INTERFACE_DISABLED != 0
            || self.host_transmitting.get()
    }

    pub async fn run(&self, clock: &impl Clock, keyboard: async_channel::Sender<u8>) {
        // Retain one deferred response separately from the unread output latch.
        // A later reply-producing command replaces it; the unread latch stays intact.
        let mut reply = None;
        loop {
            let mut command = match self.receive(&mut reply).await {
                Input::Command(command) => command,
                Input::Data(value) => {
                    // A host request takes the wire from an unfinished scan or reply.
                    self.transmission.set(None);
                    self.host_transmitting.set(true);
                    self.changed.notify(usize::MAX);
                    clock.sleep(SERIAL_FRAME_TIME).await;
                    keyboard
                        .send(value)
                        .await
                        .expect("keyboard service lives with controller");
                    self.host_transmitting.set(false);
                    self.changed.notify(usize::MAX);
                    continue;
                }
            };
            loop {
                match command {
                    READ_COMMAND_BYTE => {
                        let value = self.registers.borrow().command_byte;
                        reply = Some(value);
                    }
                    WRITE_COMMAND_BYTE | WRITE_OUTPUT_BUFFER => {
                        let value = match self.receive(&mut reply).await {
                            Input::Data(value) => value,
                            Input::Command(replacement) => {
                                command = replacement;
                                continue;
                            }
                        };
                        if command == WRITE_COMMAND_BYTE {
                            let mut registers = self.registers.borrow_mut();
                            if (registers.command_byte ^ value) & TRANSLATE_SET_2 != 0 {
                                registers.break_prefix = false;
                            }
                            registers.command_byte = value;
                            if value & INTERFACE_DISABLED != 0 {
                                self.transmission.set(None);
                            }
                        } else {
                            reply = Some(value);
                        }
                    }
                    SELF_TEST => {
                        self.registers.borrow_mut().command_byte |= SYSTEM_INITIALIZED;
                        reply = Some(0x55);
                    }
                    INTERFACE_TEST => reply = Some(0),
                    DISABLE_INTERFACE => {
                        self.registers.borrow_mut().command_byte |= INTERFACE_DISABLED;
                        self.transmission.set(None);
                    }
                    ENABLE_INTERFACE => {
                        self.registers.borrow_mut().command_byte &= !INTERFACE_DISABLED
                    }
                    _ => {}
                }
                self.update_irq();
                self.changed.notify(usize::MAX);
                break;
            }
        }
    }

    fn update_irq(&self) {
        let registers = self.registers.borrow();
        self.irq
            .set_level(registers.output_full && registers.command_byte & IRQ_ENABLED != 0);
    }

    async fn receive(&self, reply: &mut Option<u8>) -> Input {
        while let Some(byte) = *reply {
            // Input commands remain live while output is occupied, including
            // BIOS inhibition and the parameter phase of a controller command.
            match select(
                pin!(self.receive_input()),
                pin!(self.send(byte, Source::Controller)),
            )
            .await
            {
                Either::Left((input, _)) => return input,
                Either::Right(((), _)) => *reply = None,
            }
        }
        self.receive_input().await
    }

    async fn receive_input(&self) -> Input {
        loop {
            let changed = self.changed.listen();
            if let Some(input) = self.registers.borrow_mut().input.take() {
                self.changed.notify(usize::MAX);
                return input;
            }
            changed.await;
        }
    }

    pub async fn send_keyboard(&self, byte: u8, clock: &impl Clock) {
        loop {
            let changed = self.changed.listen();
            if self.inhibited() {
                self.transmission.set(None);
                changed.await;
                continue;
            }
            let transmission = self.transmission.get().unwrap_or_else(|| {
                let transmission = Transmission {
                    byte,
                    deadline: clock.now().saturating_add(SERIAL_FRAME_TIME),
                };
                self.transmission.set(Some(transmission));
                transmission
            });
            assert_eq!(transmission.byte, byte, "one keyboard transmitter");
            if clock.now() >= transmission.deadline {
                self.transmission.set(None);
                self.send(byte, Source::Keyboard).await;
                return;
            }
            futures_util::future::select(
                std::pin::pin!(clock.wait_until(transmission.deadline)),
                std::pin::pin!(changed),
            )
            .await;
        }
    }

    /// Reset acceptance requires the host to release the interface continuously
    /// after reading ACK. Other notifications do not restart the quiet interval.
    pub async fn released_for(&self, clock: &impl Clock, duration: Duration) {
        let mut deadline = None;
        loop {
            let changed = self.changed.listen();
            if self.inhibited() {
                deadline = None;
                changed.await;
                continue;
            }
            let deadline = *deadline.get_or_insert_with(|| clock.now().saturating_add(duration));
            if clock.now() >= deadline {
                return;
            }
            futures_util::future::select(
                std::pin::pin!(clock.wait_until(deadline)),
                std::pin::pin!(changed),
            )
            .await;
        }
    }

    async fn send(&self, mut byte: u8, source: Source) {
        loop {
            let changed = self.changed.listen();
            {
                let mut registers = self.registers.borrow_mut();
                if !registers.output_full
                    && (matches!(source, Source::Controller)
                        || registers.command_byte & INTERFACE_DISABLED == 0)
                {
                    if matches!(source, Source::Keyboard)
                        && registers.command_byte & TRANSLATE_SET_2 != 0
                    {
                        if byte == 0xf0 {
                            registers.break_prefix = true;
                            return;
                        }
                        byte = scancodes::translate(byte);
                        if std::mem::take(&mut registers.break_prefix) {
                            byte |= 0x80;
                        }
                    }
                    registers.output = byte;
                    registers.output_full = true;
                    if matches!(source, Source::Controller) {
                        self.transmission.set(None);
                    }
                    drop(registers);
                    self.update_irq();
                    self.changed.notify(usize::MAX);
                    return;
                }
            }
            changed.await;
        }
    }
}

enum Source {
    Controller,
    Keyboard,
}

#[cfg(test)]
mod tests;
