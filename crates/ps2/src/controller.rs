//! The 8042 input register, shared output latch and synchronous wire arbitration.

use crate::{
    keyboard::Keyboard, mouse::Mouse, protocol::controller::*, scancodes, AUXILIARY_OUTPUT,
    INPUT_BUFFER_FULL, OUTPUT_BUFFER_FULL,
};
use event_listener::Event;
use relocade_frontend::{keyboard::KeyboardEvent, mouse::MouseEvent};
use relocade_machine::{Alarm, Clock, Device, IoBus, Timestamp};
use relocade_pic::{IrqLine, Pic, KEYBOARD_IRQ, MOUSE_IRQ};
use std::{cell::RefCell, rc::Rc};

const COMMAND_INPUT: u8 = 1 << 3;
const KEYBOARD_UNLOCKED: u8 = 1 << 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Port {
    Keyboard,
    Auxiliary,
}

impl Port {
    fn other(self) -> Self {
        match self {
            Self::Keyboard => Self::Auxiliary,
            Self::Auxiliary => Self::Keyboard,
        }
    }

    fn disabled(self) -> u8 {
        match self {
            Self::Keyboard => KEYBOARD_DISABLED,
            Self::Auxiliary => AUXILIARY_DISABLED,
        }
    }
}

enum Input {
    Command(u8),
    Data(u8),
}

enum Parameter {
    CommandByte,
    Output(Port),
    AuxiliaryCommand,
}

enum HostEvent {
    Key(KeyboardEvent),
    Mouse(MouseEvent),
}

struct Output {
    port: Port,
    byte: u8,
}

struct Firmware {
    input: Option<Input>,
    parameter: Option<Parameter>,
    reply: Option<Output>,
    output: Output,
    output_full: bool,
    command_byte: u8,
    last_command: bool,
    break_prefix: bool,
    last_device: Port,
    keyboard: Keyboard,
    mouse: Option<Mouse>,
}

impl Device for Firmware {}

impl Firmware {
    fn status(&self) -> u8 {
        let mut status = KEYBOARD_UNLOCKED | (self.command_byte & SYSTEM_INITIALIZED);
        if self.last_command {
            status |= COMMAND_INPUT;
        }
        if self.input.is_some() {
            status |= INPUT_BUFFER_FULL;
        }
        if self.output_full {
            status |= OUTPUT_BUFFER_FULL;
            if self.output.port == Port::Auxiliary {
                status |= AUXILIARY_OUTPUT;
            }
        }
        status
    }

    fn consume(&mut self, input: Input) {
        match input {
            Input::Command(command) => self.command(command),
            Input::Data(byte) => match self.parameter.take() {
                Some(Parameter::CommandByte) => {
                    if (self.command_byte ^ byte) & TRANSLATE_SET_2 != 0 {
                        self.break_prefix = false;
                    }
                    self.command_byte = byte;
                }
                Some(Parameter::Output(port)) => self.reply = Some(Output { port, byte }),
                Some(Parameter::AuxiliaryCommand) => {
                    if let Some(mouse) = &mut self.mouse {
                        mouse.command(byte);
                    }
                }
                None => self.keyboard.command(byte),
            },
        }
    }

    fn command(&mut self, command: u8) {
        self.parameter = None;
        match command {
            READ_COMMAND_BYTE => self.reply(self.command_byte),
            WRITE_COMMAND_BYTE => self.parameter = Some(Parameter::CommandByte),
            WRITE_KEYBOARD_OUTPUT => self.parameter = Some(Parameter::Output(Port::Keyboard)),
            WRITE_AUXILIARY_OUTPUT => self.parameter = Some(Parameter::Output(Port::Auxiliary)),
            WRITE_AUXILIARY => self.parameter = Some(Parameter::AuxiliaryCommand),
            SELF_TEST => {
                self.command_byte |= SYSTEM_INITIALIZED;
                self.reply(0x55);
            }
            AUXILIARY_TEST | KEYBOARD_TEST => self.reply(0),
            ENABLE_KEYBOARD => self.command_byte &= !KEYBOARD_DISABLED,
            DISABLE_KEYBOARD => self.command_byte |= KEYBOARD_DISABLED,
            ENABLE_AUXILIARY => self.command_byte &= !AUXILIARY_DISABLED,
            DISABLE_AUXILIARY => self.command_byte |= AUXILIARY_DISABLED,
            _ => {}
        }
    }

    fn reply(&mut self, byte: u8) {
        self.reply = Some(Output {
            port: Port::Keyboard,
            byte,
        });
    }

    fn next_device_byte(&self) -> Option<Output> {
        for port in [self.last_device.other(), self.last_device] {
            if self.command_byte & port.disabled() == 0 {
                let byte = match port {
                    Port::Keyboard => self.keyboard.byte(),
                    Port::Auxiliary => self.mouse.as_ref().and_then(Mouse::byte),
                };
                if let Some(byte) = byte {
                    return Some(Output { port, byte });
                }
            }
        }
        None
    }

    fn translate(&mut self, byte: u8) -> Option<u8> {
        if self.command_byte & TRANSLATE_SET_2 == 0 {
            return Some(byte);
        }
        if byte == 0xf0 {
            self.break_prefix = true;
            return None;
        }
        Some(scancodes::translate(byte) | (u8::from(std::mem::take(&mut self.break_prefix)) << 7))
    }

    fn fill_output(&mut self) {
        while !self.output_full {
            let output = if let Some(reply) = self.reply.take() {
                reply
            } else if let Some(mut output) = self.next_device_byte() {
                match output.port {
                    Port::Keyboard => {
                        self.keyboard.sent();
                        let Some(byte) = self.translate(output.byte) else {
                            continue;
                        };
                        output.byte = byte;
                    }
                    Port::Auxiliary => self.mouse.as_mut().unwrap().sent(),
                }
                self.last_device = output.port;
                output
            } else {
                break;
            };
            self.output = output;
            self.output_full = true;
        }
    }
}

/// Shared controller registers and status notifications for firmware clients.
pub struct Controller {
    firmware: Rc<RefCell<Firmware>>,
    changed: Event,
    keyboard_irq: IrqLine,
    mouse_irq: IrqLine,
    clock: Clock,
    alarm: Option<Alarm>,
}

impl Controller {
    pub(super) fn new(clock: Clock, pic: &Pic, mouse: Option<Mouse>, alarm: Option<Alarm>) -> Self {
        Self {
            firmware: Rc::new(RefCell::new(Firmware {
                input: None,
                parameter: None,
                reply: None,
                output: Output {
                    byte: 0xff,
                    port: Port::Keyboard,
                },
                output_full: false,
                command_byte: KEYBOARD_IRQ_ENABLED
                    | SYSTEM_INITIALIZED
                    | AUXILIARY_DISABLED
                    | TRANSLATE_SET_2,
                last_command: false,
                break_prefix: false,
                last_device: Port::Auxiliary,
                keyboard: Keyboard::new(clock.clone()),
                mouse,
            })),
            changed: Event::new(),
            keyboard_irq: pic.irq(KEYBOARD_IRQ, false),
            mouse_irq: pic.irq(MOUSE_IRQ, false),
            clock,
            alarm,
        }
    }

    pub(super) fn connect_ports(self: &Rc<Self>, io: &IoBus) {
        io.connect(self.firmware.clone(), |ports| {
            crate::ports::configure(ports, self)
        });
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
        self.firmware.borrow().status()
    }

    pub(super) fn read_data(&self) -> u8 {
        let mut firmware = self.firmware.borrow_mut();
        if firmware.output_full {
            firmware.output_full = false;
            self.update_irq(&firmware);
            self.changed.notify(usize::MAX);
            self.schedule(&firmware);
        }
        firmware.output.byte
    }

    pub(super) fn write_data(&self, value: u8) {
        self.write_input(Input::Data(value));
    }

    pub(super) fn write_command(&self, value: u8) {
        self.write_input(Input::Command(value));
    }

    fn write_input(&self, input: Input) {
        let mut firmware = self.firmware.borrow_mut();
        if firmware.input.is_none() {
            firmware.last_command = matches!(input, Input::Command(_));
            firmware.input = Some(input);
            self.schedule(&firmware);
        }
    }

    pub(super) fn key_event(&self, event: KeyboardEvent) {
        self.advance(Some(HostEvent::Key(event)));
    }

    pub(super) fn mouse_event(&self, event: MouseEvent) {
        self.advance(Some(HostEvent::Mouse(event)));
    }

    /// Accepted port writes, reads that free the latch and device deadlines
    /// schedule this callback at the next machine boundary.
    pub(super) fn service(&self) {
        self.advance(None);
    }

    fn advance(&self, event: Option<HostEvent>) {
        let mut firmware = self.firmware.borrow_mut();
        let status = firmware.status();
        // An unread controller reply stalls command processing. Once it reaches
        // the latch, the controller can consume a later write even while OBF is set.
        if firmware.reply.is_some() {
            firmware.fill_output();
        }
        if firmware.reply.is_none() {
            if let Some(input) = firmware.input.take() {
                firmware.consume(input);
            }
        }
        firmware.fill_output();
        match event {
            Some(HostEvent::Key(event)) => firmware.keyboard.event(event),
            Some(HostEvent::Mouse(event)) => {
                // If a sample is already due, its header can reach the latch
                // before the new movement changes the counters.
                if let Some(mouse) = &mut firmware.mouse {
                    mouse.sample();
                }
                firmware.fill_output();
                if let Some(mouse) = &mut firmware.mouse {
                    mouse.event(event);
                }
            }
            None => {}
        }
        let inhibited = firmware.output_full || firmware.command_byte & KEYBOARD_DISABLED != 0;
        firmware.keyboard.repeat(inhibited);
        if let Some(mouse) = &mut firmware.mouse {
            mouse.sample();
        }
        firmware.fill_output();
        self.update_irq(&firmware);
        if firmware.status() != status {
            self.changed.notify(usize::MAX);
        }
        self.schedule(&firmware);
    }

    #[cfg(test)]
    pub(super) fn due(&self) -> bool {
        self.next_deadline(&self.firmware.borrow())
            .is_some_and(|when| when <= self.clock.now())
    }

    fn update_irq(&self, firmware: &Firmware) {
        self.keyboard_irq.set_level(
            firmware.output_full
                && firmware.output.port == Port::Keyboard
                && firmware.command_byte & KEYBOARD_IRQ_ENABLED != 0,
        );
        self.mouse_irq.set_level(
            firmware.output_full
                && firmware.output.port == Port::Auxiliary
                && firmware.command_byte & AUXILIARY_IRQ_ENABLED != 0,
        );
    }

    fn schedule(&self, firmware: &Firmware) {
        if let Some(alarm) = &self.alarm {
            alarm.replace(self.next_deadline(firmware));
        }
    }

    fn next_deadline(&self, firmware: &Firmware) -> Option<Timestamp> {
        if (firmware.input.is_some() && firmware.reply.is_none())
            || (!firmware.output_full
                && (firmware.reply.is_some() || firmware.next_device_byte().is_some()))
        {
            Some(self.clock.now())
        } else {
            firmware
                .keyboard
                .deadline()
                .into_iter()
                .chain(firmware.mouse.as_ref().and_then(Mouse::deadline))
                .min()
        }
    }
}

#[cfg(test)]
mod tests;
