//! 8042 data register, status, commands and set-2 translation.

use crate::{keyboard::Keyboard, scancodes};
use relocade_pic::IrqLine;

#[derive(Clone, Copy)]
enum Parameter {
    CommandByte,
    OutputBuffer,
}

pub(super) struct Controller {
    pub keyboard: Keyboard,
    irq: IrqLine,
    command_byte: u8,
    parameter: Option<Parameter>,
    output: Option<u8>,
    last_command: bool,
    break_prefix: bool,
}

impl Controller {
    pub fn new(irq: IrqLine) -> Self {
        Self {
            keyboard: Keyboard::default(),
            irq,
            command_byte: 0x45,
            parameter: None,
            output: None,
            last_command: false,
            break_prefix: false,
        }
    }

    pub fn repeat(&mut self, now: std::time::Duration) {
        self.keyboard
            .repeat(now, self.output.is_some() || self.command_byte & 0x10 != 0);
        self.refill();
    }

    pub fn status(&self) -> u8 {
        // Commands complete synchronously, so IBF and transmission errors are clear.
        0x10 | (self.command_byte & 4)
            | (u8::from(self.last_command) << 3)
            | u8::from(self.output.is_some())
    }

    pub fn read_data(&mut self) -> u8 {
        let byte = self.output.take().unwrap_or(0xff);
        self.irq.set_level(false);
        self.refill();
        byte
    }

    fn publish(&mut self, byte: u8) {
        self.output = Some(byte);
        self.irq.set_level(self.command_byte & 1 != 0);
    }

    pub fn refill(&mut self) {
        while self.output.is_none() && self.command_byte & 0x10 == 0 {
            let Some(mut byte) = self.keyboard.next_byte() else {
                break;
            };
            if self.command_byte & 0x40 != 0 {
                if byte == 0xf0 {
                    self.break_prefix = true;
                    continue;
                }
                byte = scancodes::translate(byte);
                if std::mem::take(&mut self.break_prefix) {
                    byte |= 0x80;
                }
            }
            self.publish(byte);
        }
        self.irq
            .set_level(self.output.is_some() && self.command_byte & 1 != 0);
    }

    pub fn command(&mut self, value: u8) {
        self.last_command = true;
        self.parameter = None;
        match value {
            0x20 => self.publish(self.command_byte),
            0x60 => self.parameter = Some(Parameter::CommandByte),
            0xaa => {
                self.command_byte |= 4;
                self.publish(0x55);
            }
            0xab => self.publish(0),
            0xad => self.command_byte |= 0x10,
            0xae => self.command_byte &= !0x10,
            0xd2 => self.parameter = Some(Parameter::OutputBuffer),
            _ => {}
        }
        self.refill();
    }

    pub fn data(&mut self, value: u8) {
        self.last_command = false;
        match self.parameter.take() {
            Some(Parameter::CommandByte) => {
                self.command_byte = value;
                self.break_prefix = false;
            }
            Some(Parameter::OutputBuffer) => self.publish(value),
            None => self.keyboard.command(value),
        }
        self.refill();
    }
}
