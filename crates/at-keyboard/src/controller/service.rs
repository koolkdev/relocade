//! Sequential 8042 firmware commands. Port callbacks only fill the input register.

use super::{Controller, Input, Source, INTERFACE_DISABLED, SYSTEM_INITIALIZED, TRANSLATE_SET_2};

const READ_COMMAND_BYTE: u8 = 0x20;
const WRITE_COMMAND_BYTE: u8 = 0x60;
const SELF_TEST: u8 = 0xaa;
const INTERFACE_TEST: u8 = 0xab;
const DISABLE_INTERFACE: u8 = 0xad;
const ENABLE_INTERFACE: u8 = 0xae;
const WRITE_OUTPUT_BUFFER: u8 = 0xd2;

impl Controller {
    pub async fn run(&self, keyboard: async_channel::Sender<u8>) {
        loop {
            let mut command = match self.receive().await {
                Input::Command(command) => command,
                Input::Data(value) => {
                    keyboard
                        .send(value)
                        .await
                        .expect("keyboard service lives with controller");
                    continue;
                }
            };
            loop {
                match command {
                    READ_COMMAND_BYTE => {
                        let value = self.registers.borrow().command_byte;
                        self.send(value, Source::Controller).await;
                    }
                    WRITE_COMMAND_BYTE | WRITE_OUTPUT_BUFFER => {
                        let value = match self.receive().await {
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
                        } else {
                            self.send(value, Source::Controller).await;
                        }
                    }
                    SELF_TEST => {
                        self.registers.borrow_mut().command_byte |= SYSTEM_INITIALIZED;
                        self.send(0x55, Source::Controller).await;
                    }
                    INTERFACE_TEST => self.send(0, Source::Controller).await,
                    DISABLE_INTERFACE => {
                        self.registers.borrow_mut().command_byte |= INTERFACE_DISABLED
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
}
