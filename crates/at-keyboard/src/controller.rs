//! 8042 input/output registers and keyboard wire translation.

mod service;

use crate::{scancodes, DATA_PORT, INPUT_BUFFER_FULL, OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT};
use event_listener::Event;
use relocade_pic::IrqLine;
use std::cell::RefCell;

const IRQ_ENABLED: u8 = 1;
const SYSTEM_INITIALIZED: u8 = 1 << 2;
const COMMAND_INPUT: u8 = 1 << 3;
const KEYBOARD_UNLOCKED: u8 = 1 << 4;
const INTERFACE_DISABLED: u8 = 1 << 4;
const TRANSLATE_SET_2: u8 = 1 << 6;

#[derive(Clone, Copy)]
enum Input {
    Command(u8),
    Data(u8),
}

struct Registers {
    input: Option<Input>,
    output: Option<u8>,
    command_byte: u8,
    last_command: bool,
    break_prefix: bool,
}

pub(super) struct Controller {
    registers: RefCell<Registers>,
    changed: Event,
    irq: IrqLine,
}

impl Controller {
    pub fn new(irq: IrqLine) -> Self {
        Self {
            registers: RefCell::new(Registers {
                input: None,
                output: None,
                command_byte: IRQ_ENABLED | SYSTEM_INITIALIZED | TRANSLATE_SET_2,
                last_command: false,
                break_prefix: false,
            }),
            changed: Event::new(),
            irq,
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
            | if registers.output.is_some() {
                OUTPUT_BUFFER_FULL
            } else {
                0
            }
    }

    pub fn read_data(&self) -> u8 {
        let byte = self.registers.borrow_mut().output.take().unwrap_or(0xff);
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
        registers.output.is_some() || registers.command_byte & INTERFACE_DISABLED != 0
    }

    fn update_irq(&self) {
        let registers = self.registers.borrow();
        self.irq
            .set_level(registers.output.is_some() && registers.command_byte & IRQ_ENABLED != 0);
    }

    async fn receive(&self) -> Input {
        loop {
            let changed = self.changed.listen();
            if let Some(input) = self.registers.borrow_mut().input.take() {
                return input;
            }
            changed.await;
        }
    }

    pub async fn send_keyboard(&self, byte: u8) {
        self.send(byte, Source::Keyboard).await;
    }

    async fn send(&self, mut byte: u8, source: Source) {
        loop {
            let changed = self.changed.listen();
            {
                let mut registers = self.registers.borrow_mut();
                if registers.output.is_none()
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
                    registers.output = Some(byte);
                    drop(registers);
                    self.update_irq();
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
