//! Exclusive store ownership across synchronous Wasm calls and their callbacks.
#![allow(unsafe_code)]

use super::{Memory, Store};
use crate::{WasmContext, WasmMemoryAccess};
use std::{
    cell::{Cell, RefCell, RefMut},
    ptr::NonNull,
};

pub(super) struct Owner<T> {
    pub data: RefCell<Store<T>>,
    available: Cell<Option<NonNull<Store<T>>>>,
}

impl<T> Owner<T> {
    pub fn new(store: Store<T>) -> Self {
        Self {
            data: RefCell::new(store),
            available: Cell::new(None),
        }
    }

    pub fn execution(&self) -> Execution<'_, T> {
        Execution {
            store: self.data.borrow_mut(),
            owner: self,
        }
    }

    pub fn callback(&self) -> Context<'_, T> {
        // Taking the slot excludes another callback until this context is
        // dropped. Raw calls outside an execution scope borrow the retained
        // store normally, including calls after the backend has been dropped.
        let (store, borrow) = match self.available.take() {
            Some(store) => (store, None),
            None => {
                let mut borrow = self.data.borrow_mut();
                (NonNull::from(&mut *borrow), Some(borrow))
            }
        };
        Context {
            store,
            owner: self,
            borrow,
        }
    }
}

/// Owns the backend borrow across guest calls and stopped host work. No pointer
/// is published to callbacks while this owner is suspended across an await.
pub struct Execution<'a, T> {
    store: RefMut<'a, Store<T>>,
    owner: &'a Owner<T>,
}

impl<T> Execution<'_, T> {
    pub(super) fn call<R>(&mut self, call: impl FnOnce() -> R) -> R {
        // The RefMut keeps the store alive and exclusively borrowed. While the
        // synchronous call runs, only a callback that takes this slot can use
        // the pointer. We do not access the RefMut again until it returns.
        let store = NonNull::from(&mut *self.store);
        assert!(self.owner.available.replace(Some(store)).is_none());
        let _scope = CallScope(&self.owner.available);
        call()
    }
}

struct CallScope<'a, T>(&'a Cell<Option<NonNull<Store<T>>>>);

impl<T> Drop for CallScope<'_, T> {
    fn drop(&mut self) {
        self.0.set(None);
    }
}

/// A callback's exclusive reborrow. Dropping it releases access immediately,
/// allowing a retained callback to be invoked again by another Wasm call.
pub struct Context<'a, T> {
    store: NonNull<Store<T>>,
    owner: &'a Owner<T>,
    borrow: Option<RefMut<'a, Store<T>>>,
}

impl<T> Context<'_, T> {
    fn store(&self) -> &Store<T> {
        // SAFETY: Either borrow owns the RefMut, or the synchronous execution
        // scope owns it and this context has taken its only available pointer.
        // The callback cannot outlive that call. Borrowing through &self keeps
        // this context alive and excludes a mutable borrow through it.
        unsafe { self.store.as_ref() }
    }

    fn store_mut(&mut self) -> &mut Store<T> {
        // SAFETY: The same ownership contract as store(), with exclusive access
        // through &mut self. The slot remains empty until this context drops.
        unsafe { self.store.as_mut() }
    }
}

impl<T> Drop for Context<'_, T> {
    fn drop(&mut self) {
        if self.borrow.is_none() {
            self.owner.available.set(Some(self.store));
        }
    }
}

impl<T> WasmContext for Execution<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        &self.store.state
    }

    fn state_mut(&mut self) -> &mut T {
        &mut self.store.state
    }
}

impl<T> WasmMemoryAccess for Execution<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        self.store.read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        self.store.write_memory(memory, offset, bytes);
    }
}

impl<T> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        &self.store().state
    }

    fn state_mut(&mut self) -> &mut T {
        &mut self.store_mut().state
    }
}

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        self.store().read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        self.store_mut().write_memory(memory, offset, bytes);
    }
}
