//! Physical key changes awaiting the keyboard's next scan.

use crate::scancodes;
use futures_util::StreamExt;
use relocade_frontend::keyboard::{
    ElementState, KeyCode, KeyboardEvent, KeyboardEvents, PhysicalKey,
};
use relocade_machine::{Clock, Timestamp};
use std::{
    collections::{BTreeSet, VecDeque},
    future::pending,
};

pub(super) struct Matrix {
    events: Option<KeyboardEvents>,
    // Pending matrix changes from a snapshot or command rescan.
    changes: VecDeque<(KeyCode, bool)>,
    pub(super) held: BTreeSet<KeyCode>,
}

impl Matrix {
    pub(super) fn new(events: KeyboardEvents) -> Self {
        Self {
            events: Some(events),
            changes: VecDeque::new(),
            held: BTreeSet::new(),
        }
    }

    pub(super) async fn next(
        &mut self,
        clock: &Clock,
        ready_at: Timestamp,
    ) -> Option<(KeyCode, bool)> {
        if self.changes.is_empty() {
            let Some(events) = &mut self.events else {
                return pending().await;
            };
            let event = events.next().await;
            let event = match event {
                Some(input) => input.event,
                None => {
                    self.events = None;
                    KeyboardEvent::FocusLost
                }
            };
            self.queue(event);
        }
        let &(key, pressed) = self.changes.front()?;
        // Retain the change if a command, output byte or repeat wins the selection.
        // An idle input channel never arms a scan timer.
        if ready_at > clock.now() {
            clock.sleep_until(ready_at).await;
        }
        self.changes.pop_front();
        if pressed {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        Some((key, pressed))
    }

    fn queue(&mut self, event: KeyboardEvent) {
        use KeyCode::*;
        match event {
            KeyboardEvent::Key {
                physical_key: PhysicalKey::Code(key),
                state,
                ..
            } if scancodes::supported(key) => {
                self.change(key, state == ElementState::Pressed);
            }
            KeyboardEvent::FocusLost => {
                self.changes
                    .extend(self.held.iter().map(|&key| (key, false)));
            }
            KeyboardEvent::Modifiers(modifiers) => {
                for (active, left, right) in [
                    (modifiers.shift_key(), ShiftLeft, ShiftRight),
                    (modifiers.control_key(), ControlLeft, ControlRight),
                    (modifiers.alt_key(), AltLeft, AltRight),
                    (modifiers.super_key(), SuperLeft, SuperRight),
                ] {
                    if !active {
                        self.change(left, false);
                        self.change(right, false);
                    } else if !self.held.contains(&right) {
                        self.change(left, true);
                    }
                }
            }
            _ => {}
        }
    }

    fn change(&mut self, key: KeyCode, pressed: bool) {
        if self.held.contains(&key) != pressed {
            self.changes.push_back((key, pressed));
        }
    }

    /// Scan differences accumulated during a command before any newer input.
    pub(super) fn rescan(&mut self, before: BTreeSet<KeyCode>) {
        if self.held == before {
            return;
        }
        let later = std::mem::take(&mut self.changes);
        self.changes
            .extend(before.difference(&self.held).map(|&key| (key, false)));
        self.changes
            .extend(self.held.difference(&before).map(|&key| (key, true)));
        self.changes.extend(later);
        self.held = before;
    }
}
