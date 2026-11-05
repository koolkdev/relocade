//! Keyboard command conversations, servicing physical input during wire waits.

use super::Keyboard;
use crate::controller::Controller;
use futures_util::{pin_mut, select_biased, FutureExt};
use relocade_frontend::keyboard::{KeyboardEvent, KeyboardEvents};
use relocade_machine::Clock;
use std::future::pending;

const SET_LEDS: u8 = 0xed;
const ECHO: u8 = 0xee;
const SCAN_SET: u8 = 0xf0;
const IDENTIFY: u8 = 0xf2;
const SET_TYPEMATIC: u8 = 0xf3;
const ENABLE_SCANNING: u8 = 0xf4;
const DEFAULT_DISABLE: u8 = 0xf5;
const DEFAULTS: u8 = 0xf6;
const ACK: u8 = 0xfa;
const RESEND: u8 = 0xfe;
const RESET: u8 = 0xff;

pub(crate) async fn run<C: Clock>(
    clock: C,
    events: KeyboardEvents,
    controller: &Controller,
    commands: async_channel::Receiver<u8>,
) {
    Service {
        clock,
        events: Some(events),
        controller,
        commands,
        keyboard: Keyboard::default(),
        last_sent: RESEND,
    }
    .run()
    .await;
}

struct Service<'a, C> {
    clock: C,
    events: Option<KeyboardEvents>,
    controller: &'a Controller,
    commands: async_channel::Receiver<u8>,
    keyboard: Keyboard,
    last_sent: u8,
}

impl<C: Clock> Service<'_, C> {
    async fn run(mut self) {
        let mut command = self.receive().await;
        loop {
            let result = self.execute(command).await;
            self.keyboard.resume(self.clock.now());
            command = match result {
                Err(CommandReplaced(command)) => command,
                Ok(()) => self.receive().await,
            };
        }
    }

    async fn execute(&mut self, command: u8) -> Result<(), CommandReplaced> {
        match command {
            SET_LEDS | SCAN_SET | SET_TYPEMATIC => self.configure(command).await?,
            ECHO => self.reply(ECHO).await?,
            IDENTIFY => {
                self.keyboard.pause();
                for byte in [ACK, 0xab, 0x83] {
                    self.reply(byte).await?;
                }
            }
            ENABLE_SCANNING => {
                self.keyboard.clear_scans();
                self.keyboard.scanning = true;
                self.reply(ACK).await?;
            }
            DEFAULT_DISABLE | DEFAULTS => {
                self.keyboard.defaults(command != DEFAULT_DISABLE);
                self.reply(ACK).await?;
            }
            RESET => {
                self.keyboard.pause();
                self.reply(ACK).await?;
                self.keyboard.defaults(true);
                self.reply(0xaa).await?;
            }
            RESEND => self.reply(self.last_sent).await?,
            _ => self.reply(RESEND).await?,
        }
        Ok(())
    }

    /// A new command replaces the unfinished option exchange. RESEND leaves it
    /// waiting for the option, and invalid options request another attempt.
    async fn configure(&mut self, command: u8) -> Result<(), CommandReplaced> {
        self.keyboard.pause();
        if command == SCAN_SET {
            self.keyboard.clear_scans();
        }
        self.reply(ACK).await?;
        loop {
            let value = self.receive().await;
            if value == RESEND {
                self.reply(self.last_sent).await?;
                continue;
            }
            if value >= SET_LEDS {
                return Err(CommandReplaced(value));
            }
            match command {
                SET_LEDS if value < 8 => self.keyboard.leds = value,
                SET_TYPEMATIC if value < 128 => self.keyboard.typematic = value,
                SCAN_SET if value == 0 => {
                    self.reply(ACK).await?;
                    self.reply(self.keyboard.set).await?;
                    return Ok(());
                }
                SCAN_SET if value <= 2 => self.keyboard.set = value,
                _ => {
                    self.reply(RESEND).await?;
                    continue;
                }
            }
            self.reply(ACK).await?;
            return Ok(());
        }
    }

    async fn receive(&mut self) -> u8 {
        match self.transfer(Transfer::Receive).await {
            Ready::Command(value) => value,
            Ready::Sent => unreachable!("receive returns a command"),
        }
    }

    async fn reply(&mut self, byte: u8) -> Result<(), CommandReplaced> {
        match self.transfer(Transfer::Reply(byte)).await {
            Ready::Sent => Ok(()),
            Ready::Command(command) => Err(CommandReplaced(command)),
        }
    }

    /// While receiving commands, drain queued scans. Replies take priority over
    /// scans; a new command cancels a byte that has not reached the controller.
    /// Every wait also tracks physical key transitions and repeat deadlines.
    async fn transfer(&mut self, transfer: Transfer) -> Ready {
        loop {
            let outgoing = match transfer {
                Transfer::Reply(byte) => Some(byte),
                Transfer::Receive if self.keyboard.paused.is_none() => self.keyboard.peek_byte(),
                Transfer::Receive => None,
            };
            let wake = {
                let command = self.commands.recv().fuse();
                let output = async {
                    if let Some(byte) = outgoing {
                        self.controller.send_keyboard(byte).await;
                    } else {
                        pending::<()>().await;
                    }
                }
                .fuse();
                let event = async {
                    match self.events.as_mut() {
                        Some(events) => events.recv().await.ok(),
                        None => pending().await,
                    }
                }
                .fuse();
                let timer = async {
                    if let Some(deadline) = self.keyboard.deadline {
                        self.clock.wait_until(deadline).await;
                    } else {
                        pending::<()>().await;
                    }
                }
                .fuse();
                pin_mut!(command, output, event, timer);
                select_biased! {
                    value = command => Wake::Command(value.expect("controller service lives with keyboard")),
                    () = output => Wake::Sent,
                    event = event => Wake::Event(event),
                    () = timer => Wake::Repeat,
                }
            };
            match wake {
                Wake::Command(value) => return Ready::Command(value),
                Wake::Sent => {
                    let byte = outgoing.expect("completed keyboard transmission");
                    if byte != RESEND {
                        self.last_sent = byte;
                    }
                    if matches!(transfer, Transfer::Reply(_)) {
                        return Ready::Sent;
                    }
                    self.keyboard.consume_byte();
                }
                Wake::Event(event) => {
                    if event.is_none() {
                        self.events = None;
                    }
                    self.keyboard
                        .event(event.unwrap_or(KeyboardEvent::FocusLost), self.clock.now());
                }
                Wake::Repeat => self
                    .keyboard
                    .repeat(self.clock.now(), self.controller.inhibited()),
            }
        }
    }
}

struct CommandReplaced(u8);

#[derive(Clone, Copy)]
enum Transfer {
    Receive,
    Reply(u8),
}

enum Ready {
    Command(u8),
    Sent,
}

enum Wake {
    Command(u8),
    Event(Option<KeyboardEvent>),
    Repeat,
    Sent,
}
