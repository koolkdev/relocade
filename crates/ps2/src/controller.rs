//! Shared 8042 registers, command routing and keyboard wire translation.

use crate::{
    protocol::controller::*, scancodes, AUXILIARY_OUTPUT, INPUT_BUFFER_FULL, OUTPUT_BUFFER_FULL,
};
use event_listener::Event;
use relocade_pic::{IrqLine, Pic, KEYBOARD_IRQ, MOUSE_IRQ};
use std::cell::RefCell;

const COMMAND_INPUT: u8 = 1 << 3;
const KEYBOARD_UNLOCKED: u8 = 1 << 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Port {
    Keyboard = 0,
    Auxiliary = 1,
}

impl Port {
    fn irq_enabled(self) -> u8 {
        match self {
            Self::Keyboard => KEYBOARD_IRQ_ENABLED,
            Self::Auxiliary => AUXILIARY_IRQ_ENABLED,
        }
    }
    fn disabled(self) -> u8 {
        match self {
            Self::Keyboard => KEYBOARD_DISABLED,
            Self::Auxiliary => AUXILIARY_DISABLED,
        }
    }
    fn other(self) -> Self {
        match self {
            Self::Keyboard => Self::Auxiliary,
            Self::Auxiliary => Self::Keyboard,
        }
    }
}

#[derive(Clone, Copy)]
enum Input {
    Command(u8),
    Data(u8),
}

struct Output {
    byte: u8,
    port: Port,
}

struct Registers {
    input: Option<Input>,
    output: Output,
    output_full: bool,
    command_byte: u8,
    last_command: bool,
    break_prefix: bool,
    waiting: [bool; 2],
    last_device: Port,
}

/// Shared controller state and readiness notifications for firmware on this worker.
/// Guest and BIOS transfers still use the connected port registers.
pub struct Controller {
    registers: RefCell<Registers>,
    changed: Event,
    irqs: [IrqLine; 2],
    commands: [Option<(async_channel::Sender<u8>, async_channel::Receiver<u8>)>; 2],
}

impl Controller {
    pub(super) fn new(pic: &Pic, connected: impl IntoIterator<Item = Port>) -> Self {
        let mut commands = [None, None];
        for port in connected {
            commands[port as usize] = Some(async_channel::bounded(1));
        }
        Self {
            registers: RefCell::new(Registers {
                input: None,
                // The initial latch contents are unspecified by hardware.
                output: Output {
                    byte: 0xff,
                    port: Port::Keyboard,
                },
                output_full: false,
                command_byte: Port::Keyboard.irq_enabled()
                    | SYSTEM_INITIALIZED
                    | Port::Auxiliary.disabled()
                    | TRANSLATE_SET_2,
                last_command: false,
                break_prefix: false,
                waiting: [false; 2],
                last_device: Port::Auxiliary,
            }),
            changed: Event::new(),
            irqs: [pic.irq(KEYBOARD_IRQ, false), pic.irq(MOUSE_IRQ, false)],
            commands,
        }
    }

    /// Waits until the controller can accept another port write.
    pub async fn wait_input_ready(&self) {
        self.wait_status(INPUT_BUFFER_FULL, 0).await;
    }

    /// Waits for an unread auxiliary byte without consuming keyboard data.
    pub async fn wait_auxiliary_output(&self) {
        let mask = OUTPUT_BUFFER_FULL | AUXILIARY_OUTPUT;
        self.wait_status(mask, mask).await;
    }

    async fn wait_status(&self, mask: u8, value: u8) {
        loop {
            let changed = self.changed.listen();
            if self.status() & mask == value {
                return;
            }
            changed.await;
        }
    }

    pub(super) fn status(&self) -> u8 {
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
                    | if registers.output.port == Port::Auxiliary {
                        AUXILIARY_OUTPUT
                    } else {
                        0
                    }
            } else {
                0
            }
    }

    pub(super) fn read_data(&self) -> u8 {
        let byte = {
            let mut registers = self.registers.borrow_mut();
            // A read acknowledges output without erasing the data latch.
            registers.output_full = false;
            registers.output.byte
        };
        self.update_irq();
        self.changed.notify(usize::MAX);
        byte
    }

    pub(super) fn write_data(&self, value: u8) {
        self.write_input(Input::Data(value));
    }

    pub(super) fn write_command(&self, value: u8) {
        self.write_input(Input::Command(value));
    }

    fn write_input(&self, input: Input) {
        {
            let mut registers = self.registers.borrow_mut();
            // Software must wait for IBF to clear; busy writes cannot replace it.
            if registers.input.is_some() {
                return;
            }
            registers.input = Some(input);
            registers.last_command = matches!(input, Input::Command(_));
        }
        self.changed.notify(usize::MAX);
    }

    pub(super) fn inhibited(&self, port: Port) -> bool {
        let registers = self.registers.borrow();
        registers.output_full || registers.command_byte & port.disabled() != 0
    }

    pub(super) async fn receive_device(&self, port: Port) -> u8 {
        self.commands[port as usize]
            .as_ref()
            .expect("device must be connected")
            .1
            .recv()
            .await
            .expect("controller owns both command channels")
    }

    async fn command_device(&self, port: Port, value: u8) {
        // An empty physical port accepts the controller write but never replies.
        // It must not stop firmware from servicing the other port.
        if let Some((commands, _)) = &self.commands[port as usize] {
            commands
                .send(value)
                .await
                .expect("controller owns the receiver");
        }
    }

    pub(super) async fn run(&self) {
        loop {
            let mut command = match self.receive().await {
                Input::Command(command) => command,
                Input::Data(value) => {
                    self.command_device(Port::Keyboard, value).await;
                    continue;
                }
            };
            loop {
                match command {
                    READ_COMMAND_BYTE => {
                        let value = self.registers.borrow().command_byte;
                        self.send_output(value, Source::Controller(Port::Keyboard))
                            .await;
                    }
                    WRITE_COMMAND_BYTE
                    | WRITE_KEYBOARD_OUTPUT
                    | WRITE_AUXILIARY_OUTPUT
                    | WRITE_AUXILIARY => {
                        let value = match self.receive().await {
                            Input::Data(value) => value,
                            Input::Command(replacement) => {
                                command = replacement;
                                continue;
                            }
                        };
                        match command {
                            WRITE_COMMAND_BYTE => {
                                let mut registers = self.registers.borrow_mut();
                                if (registers.command_byte ^ value) & TRANSLATE_SET_2 != 0 {
                                    registers.break_prefix = false;
                                }
                                registers.command_byte = value;
                            }
                            WRITE_KEYBOARD_OUTPUT => {
                                self.send_output(value, Source::Controller(Port::Keyboard))
                                    .await
                            }
                            WRITE_AUXILIARY_OUTPUT => {
                                self.send_output(value, Source::Controller(Port::Auxiliary))
                                    .await
                            }
                            WRITE_AUXILIARY => self.command_device(Port::Auxiliary, value).await,
                            _ => unreachable!(),
                        }
                    }
                    SELF_TEST => {
                        self.registers.borrow_mut().command_byte |= SYSTEM_INITIALIZED;
                        self.send_output(0x55, Source::Controller(Port::Keyboard))
                            .await;
                    }
                    AUXILIARY_TEST | KEYBOARD_TEST => {
                        self.send_output(0, Source::Controller(Port::Keyboard))
                            .await
                    }
                    DISABLE_AUXILIARY => {
                        self.registers.borrow_mut().command_byte |= Port::Auxiliary.disabled()
                    }
                    ENABLE_AUXILIARY => {
                        self.registers.borrow_mut().command_byte &= !Port::Auxiliary.disabled()
                    }
                    DISABLE_KEYBOARD => {
                        self.registers.borrow_mut().command_byte |= Port::Keyboard.disabled()
                    }
                    ENABLE_KEYBOARD => {
                        self.registers.borrow_mut().command_byte &= !Port::Keyboard.disabled()
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
        for port in [Port::Keyboard, Port::Auxiliary] {
            self.irqs[port as usize].set_level(
                registers.output_full
                    && registers.output.port == port
                    && registers.command_byte & port.irq_enabled() != 0,
            );
        }
    }

    async fn receive(&self) -> Input {
        loop {
            let changed = self.changed.listen();
            let input = self.registers.borrow_mut().input.take();
            if let Some(input) = input {
                self.changed.notify(usize::MAX);
                return input;
            }
            changed.await;
        }
    }

    pub(super) async fn send(&self, port: Port, byte: u8) {
        self.send_output(byte, Source::Device(port)).await;
    }

    async fn send_output(&self, mut byte: u8, source: Source) {
        // A cancelled transmission must stop participating in arbitration.
        let _waiting = match source {
            Source::Device(port) => {
                self.registers.borrow_mut().waiting[port as usize] = true;
                Some(Waiting {
                    controller: self,
                    port,
                })
            }
            Source::Controller(_) => None,
        };
        loop {
            let changed = self.changed.listen();
            {
                let mut registers = self.registers.borrow_mut();
                let port = source.port();
                let allowed = match source {
                    Source::Controller(_) => true,
                    Source::Device(_) => {
                        let other = port.other();
                        let other_waiting = registers.waiting[other as usize]
                            && registers.command_byte & other.disabled() == 0;
                        registers.command_byte & port.disabled() == 0
                            && (!other_waiting || registers.last_device != port)
                    }
                };
                if !registers.output_full && allowed {
                    if matches!(source, Source::Device(Port::Keyboard))
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
                    registers.output = Output { byte, port };
                    registers.output_full = true;
                    if matches!(source, Source::Device(_)) {
                        registers.last_device = port;
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

#[derive(Clone, Copy)]
enum Source {
    Controller(Port),
    Device(Port),
}

impl Source {
    fn port(self) -> Port {
        match self {
            Self::Controller(port) | Self::Device(port) => port,
        }
    }
}

struct Waiting<'a> {
    controller: &'a Controller,
    port: Port,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.controller.registers.borrow_mut().waiting[self.port as usize] = false;
        self.controller.changed.notify(usize::MAX);
    }
}

#[cfg(test)]
mod tests;
