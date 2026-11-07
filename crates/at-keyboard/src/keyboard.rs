//! Keyboard commands, physical scanning, bounded buffering and typematic timing.

use crate::{controller::Controller, scancodes};
use futures_util::{pin_mut, select_biased, FutureExt};
use relocade_frontend::keyboard::{
    ElementState, KeyCode, KeyboardEvent, KeyboardEvents, PhysicalKey,
};
use relocade_machine::Clock;
use std::{
    collections::{BTreeSet, VecDeque},
    future::{pending, Future},
    time::Duration,
};

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
const SELF_TEST_PASSED: u8 = 0xaa;
// IBM 101/102-key keyboard: at least 500 us of host release accepts reset ACK;
// BAT takes 300–500 ms. Use the midpoint as this model's nominal duration.
const RESET_ACK_RELEASE: Duration = Duration::from_micros(500);
const SELF_TEST_DURATION: Duration = Duration::from_millis(400);

pub(super) async fn run<C: Clock>(
    clock: &C,
    events: KeyboardEvents,
    controller: &Controller,
    commands: async_channel::Receiver<u8>,
) {
    Keyboard {
        clock,
        events: Some(events),
        controller,
        commands,
        last_sent: RESEND,
        held: BTreeSet::new(),
        scans: VecDeque::new(),
        overflow: false,
        paused: None,
        scanning: true,
        set: 2,
        leds: 0,
        typematic: 0x2b,
        repeat: None,
    }
    .run()
    .await;
}

struct Keyboard<'a, C> {
    clock: &'a C,
    events: Option<KeyboardEvents>,
    controller: &'a Controller,
    commands: async_channel::Receiver<u8>,
    last_sent: u8,
    held: BTreeSet<KeyCode>,
    scans: VecDeque<u8>,
    overflow: bool,
    // Physical keys at the last scan before a command suspended scanning.
    paused: Option<BTreeSet<KeyCode>>,
    scanning: bool,
    set: u8,
    leds: u8,
    typematic: u8,
    repeat: Option<Repeat>,
}

#[derive(Clone, Copy)]
struct Repeat {
    key: KeyCode,
    deadline: Duration,
}

impl<C: Clock> Keyboard<'_, C> {
    async fn run(mut self) {
        let mut command = self.receive().await;
        loop {
            let result = self.execute(command).await;
            self.resume();
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
                self.pause();
                for byte in [ACK, 0xab, 0x83] {
                    self.reply(byte).await?;
                }
            }
            ENABLE_SCANNING => {
                self.clear_scans();
                self.scanning = true;
                self.reply(ACK).await?;
            }
            DEFAULT_DISABLE | DEFAULTS => {
                self.defaults(command != DEFAULT_DISABLE);
                self.reply(ACK).await?;
            }
            RESET => self.reset().await?,
            RESEND => self.reply(self.last_sent).await?,
            _ => self.reply(RESEND).await?,
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<(), CommandReplaced> {
        self.pause();
        self.reply(ACK).await?;
        let clock = self.clock;
        self.wait_for(self.controller.released_for(clock, RESET_ACK_RELEASE))
            .await?;
        self.defaults(false);
        let completed = clock.now().saturating_add(SELF_TEST_DURATION);
        // The keyboard ignores commands during BAT. Continue observing physical
        // state so its first scan after the test reports keys that remain held.
        while self.wait_for(clock.wait_until(completed)).await.is_err() {}
        while self.commands.try_recv().is_ok() {}
        // BAT succeeded even if a new command replaces its pending completion
        // byte. Keep scanning paused until that reply or command takeover ends.
        self.scanning = true;
        self.paused = Some(BTreeSet::new());
        self.reply(SELF_TEST_PASSED).await?;
        Ok(())
    }

    async fn wait_for(&mut self, ready: impl Future<Output = ()>) -> Result<(), CommandReplaced> {
        let ready = ready.fuse();
        pin_mut!(ready);
        loop {
            let event = {
                let command = self.commands.recv().fuse();
                let event = next_event(&mut self.events).fuse();
                pin_mut!(command, event);
                select_biased! {
                    () = ready => return Ok(()),
                    command = command => return Err(CommandReplaced(command.expect("controller service lives with keyboard"))),
                    event = event => event,
                }
            };
            self.event(event);
        }
    }

    /// A new command replaces the unfinished option exchange. RESEND leaves it
    /// waiting for the option, and invalid options request another attempt.
    async fn configure(&mut self, command: u8) -> Result<(), CommandReplaced> {
        self.pause();
        if command == SCAN_SET {
            self.clear_scans();
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
                SET_LEDS if value < 8 => self.leds = value,
                SET_TYPEMATIC if value < 128 => self.typematic = value,
                SCAN_SET if value == 0 => {
                    self.reply(ACK).await?;
                    self.reply(self.set).await?;
                    return Ok(());
                }
                SCAN_SET if value <= 2 => self.set = value,
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
        loop {
            if let Err(CommandReplaced(command)) = self.transfer(None).await {
                return command;
            }
        }
    }

    async fn reply(&mut self, byte: u8) -> Result<(), CommandReplaced> {
        self.transfer(Some(byte)).await
    }

    /// While receiving commands, drain queued scans. Replies take priority over
    /// scans; a new command cancels a byte that has not reached the controller.
    /// Every wait also tracks physical key transitions and repeat deadlines.
    async fn transfer(&mut self, reply: Option<u8>) -> Result<(), CommandReplaced> {
        loop {
            let outgoing = match reply {
                Some(byte) => Some(byte),
                None if self.paused.is_none() => self.peek_byte(),
                None => None,
            };
            let wake = {
                let command = self.commands.recv().fuse();
                let output = async {
                    if let Some(byte) = outgoing {
                        self.controller.send_keyboard(byte, self.clock).await;
                    } else {
                        pending::<()>().await;
                    }
                }
                .fuse();
                let event = next_event(&mut self.events).fuse();
                let timer = async {
                    if let Some(repeat) = self.repeat {
                        self.clock.wait_until(repeat.deadline).await;
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
                Wake::Command(value) => return Err(CommandReplaced(value)),
                Wake::Sent => {
                    let byte = outgoing.expect("completed keyboard transmission");
                    if byte != RESEND {
                        self.last_sent = byte;
                    }
                    if reply.is_none() {
                        self.consume_byte();
                    }
                    return Ok(());
                }
                Wake::Event(event) => self.event(event),
                Wake::Repeat => self.repeat(),
            }
        }
    }

    fn event(&mut self, event: Option<KeyboardEvent>) {
        if event.is_none() {
            self.events = None;
        }
        use KeyCode::*;
        let now = self.clock.now();
        match event.unwrap_or(KeyboardEvent::FocusLost) {
            KeyboardEvent::Key {
                physical_key: PhysicalKey::Code(key),
                state,
                ..
            } => {
                self.key(key, state == ElementState::Pressed, now);
            }
            KeyboardEvent::FocusLost => {
                for key in self.held.clone() {
                    self.key(key, false, now);
                }
            }
            KeyboardEvent::Modifiers(modifiers) => {
                for (active, left, right) in [
                    (modifiers.shift_key(), ShiftLeft, ShiftRight),
                    (modifiers.control_key(), ControlLeft, ControlRight),
                    (modifiers.alt_key(), AltLeft, AltRight),
                    (modifiers.super_key(), SuperLeft, SuperRight),
                ] {
                    if !active {
                        self.key(left, false, now);
                        self.key(right, false, now);
                    } else if !self.held.contains(&left) && !self.held.contains(&right) {
                        self.key(left, true, now);
                    }
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, key: KeyCode, pressed: bool, now: Duration) {
        if !scancodes::supported(key) {
            return;
        }
        let changed = if pressed {
            self.held.insert(key)
        } else {
            self.held.remove(&key)
        };
        if !changed {
            return;
        } // The guest's typematic settings own repetition.
        if self.scanning && self.paused.is_none() {
            self.scan(key, pressed, now);
        }
        if !pressed && self.repeat.is_some_and(|repeat| repeat.key == key) {
            self.repeat = None;
        }
    }

    fn scan(&mut self, key: KeyCode, pressed: bool, now: Duration) {
        self.emit(key, pressed);
        if pressed && scancodes::repeatable(key) {
            self.repeat = Some(Repeat {
                key,
                deadline: now.saturating_add(self.delay()),
            });
        }
    }

    fn pause(&mut self) {
        if self.paused.is_none() {
            self.paused = Some(self.held.clone());
        }
        self.repeat = None;
    }

    fn resume(&mut self) {
        let now = self.clock.now();
        if let Some(before) = self.paused.take() {
            if self.scanning {
                // Rescan the physical matrix: a release during an option or ID
                // exchange must not leave a previously reported modifier stuck.
                let held = self.held.clone();
                for key in before.difference(&held) {
                    self.scan(*key, false, now);
                }
                for key in held.difference(&before) {
                    self.scan(*key, true, now);
                }
            }
        }
    }

    fn emit(&mut self, key: KeyCode, pressed: bool) {
        let bytes = scancodes::sequence(key, pressed, self.set, &self.held);
        // Admit whole sequences so an overflow cannot leave a dangling prefix.
        if self.scans.len() + bytes.len() <= 16 {
            self.scans.extend(bytes);
        } else {
            self.overflow = true;
        }
    }

    fn delay(&self) -> Duration {
        Duration::from_millis(250 * (u64::from((self.typematic >> 5) & 3) + 1))
    }

    fn repeat(&mut self) {
        let now = self.clock.now();
        if let Some(repeat) = self.repeat.filter(|repeat| now >= repeat.deadline) {
            if !self.controller.inhibited() {
                self.emit(repeat.key, true);
            }
            // Rate = 240 / ((8 + A) * 2^B) Hz, per the keyboard command.
            let clocks = u64::from(8 + (self.typematic & 7)) << ((self.typematic >> 3) & 3);
            let period = Duration::from_nanos((clocks * 1_000_000_000).div_ceil(240));
            self.repeat = Some(Repeat {
                deadline: now.saturating_add(period),
                ..repeat
            });
        }
    }

    fn clear_scans(&mut self) {
        self.scans.clear();
        self.overflow = false;
        self.repeat = None;
    }

    fn defaults(&mut self, scanning: bool) {
        self.clear_scans();
        self.scanning = scanning;
        self.set = 2;
        self.typematic = 0x2b;
        self.leds = 0;
    }

    fn peek_byte(&self) -> Option<u8> {
        self.scans.front().copied().or_else(|| {
            self.overflow
                .then_some(if self.set == 1 { 0xff } else { 0 })
        })
    }

    fn consume_byte(&mut self) {
        if self.scans.pop_front().is_none() {
            self.overflow = false;
        }
    }
}

struct CommandReplaced(u8);

async fn next_event(events: &mut Option<KeyboardEvents>) -> Option<KeyboardEvent> {
    match events {
        Some(events) => events.recv().await.ok(),
        None => pending().await,
    }
}

enum Wake {
    Command(u8),
    Event(Option<KeyboardEvent>),
    Repeat,
    Sent,
}
