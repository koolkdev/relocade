//! Reuses engine callbacks while bound operations and their captures change.

use std::{collections::HashMap, rc::Rc};

struct Slot<H: ?Sized, F> {
    handler: Option<Rc<H>>,
    function: F,
    users: usize,
}

/// Ordinary and configuring operations use separate pools so a recycled callback
/// keeps its calling convention without testing the operation kind on each call.
pub(super) struct Bindings<H: ?Sized, F> {
    slots: Vec<Slot<H, F>>,
    handlers: HashMap<usize, usize>,
    ports: HashMap<u16, usize>,
    vacant: Vec<usize>,
}

impl<H: ?Sized, F> Default for Bindings<H, F> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            handlers: HashMap::new(),
            ports: HashMap::new(),
            vacant: Vec::new(),
        }
    }
}

impl<H: ?Sized, F: Clone> Bindings<H, F> {
    pub(super) fn handler(&self, slot: usize) -> &Rc<H> {
        self.slots[slot]
            .handler
            .as_ref()
            .expect("bound port operation")
    }

    pub(super) fn remove(&mut self, port: u16) {
        if let Some(slot) = self.ports.remove(&port) {
            let old = &mut self.slots[slot];
            old.users -= 1;
            if old.users == 0 {
                self.handlers
                    .remove(&identity(old.handler.as_ref().unwrap()));
                old.handler = None;
                self.vacant.push(slot);
            }
        }
    }

    /// One callback per distinct live operation, shared across its port range.
    /// Replacing the last user recycles its callback. Configuring callbacks hold
    /// their own operation reference while editing bindings.
    pub(super) fn set(&mut self, port: u16, handler: Rc<H>, create: impl FnOnce(usize) -> F) -> F {
        let key = identity(&handler);
        if let Some(&slot) = self.ports.get(&port) {
            if identity(self.handler(slot)) == key {
                return self.slots[slot].function.clone();
            }
            self.remove(port);
        }
        let slot = if let Some(&slot) = self.handlers.get(&key) {
            slot
        } else {
            let slot = if let Some(slot) = self.vacant.pop() {
                self.slots[slot].handler = Some(handler);
                slot
            } else {
                let slot = self.slots.len();
                self.slots.push(Slot {
                    handler: Some(handler),
                    function: create(slot),
                    users: 0,
                });
                slot
            };
            self.handlers.insert(key, slot);
            slot
        };
        self.ports.insert(port, slot);
        self.slots[slot].users += 1;
        self.slots[slot].function.clone()
    }
}

fn identity<H: ?Sized>(handler: &Rc<H>) -> usize {
    Rc::as_ptr(handler) as *const () as usize
}

#[cfg(test)]
mod tests {
    use super::Bindings;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn repeated_replacement_reuses_callbacks_and_releases_captures() {
        let mut bindings = Bindings::default();
        let created = Cell::new(0);
        let create = |slot| {
            created.set(created.get() + 1);
            slot
        };
        for value in 0..1000 {
            let handler = Rc::new(value);
            let lifetime = Rc::downgrade(&handler);
            let slot = bindings.set(0xffff, handler, create);
            assert_eq!(**bindings.handler(slot), value);
            bindings.remove(0xffff);
            assert!(lifetime.upgrade().is_none());
        }
        assert_eq!(created.get(), 1, "updates reuse the engine function");
    }
}
