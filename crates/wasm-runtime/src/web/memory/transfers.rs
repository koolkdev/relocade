//! Direct scalar access and copies between Rust and one imported engine memory.

mod encode;

use encode::encode;

use super::super::{js_error, set};
use crate::{Error, MemoryValue, Result};
use js_sys::{Object, SharedArrayBuffer, Uint8Array, WebAssembly};
use relocade_wasm_bindings::Function as Binding;
use std::{cell::RefCell, collections::HashMap};
use wasm_bindgen::JsCast;

pub struct Transfers {
    read_u8: Binding<(u32,), u8>,
    write_u8: Binding<(u32, u8), ()>,
    read_u16: Binding<(u32,), u16>,
    write_u16: Binding<(u32, u16), ()>,
    read_u32: Binding<(u32,), u32>,
    write_u32: Binding<(u32, u32), ()>,
    read_u64: Binding<(u32,), u64>,
    write_u64: Binding<(u32, u64), ()>,
    read: Binding<(i32, i32, i32), ()>,
    write: Binding<(i32, i32, i32), ()>,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct MemoryTypes {
    rust_shared: bool,
    imported_shared: bool,
}

impl Transfers {
    pub(super) fn new(memory: &WebAssembly::Memory, shared: bool) -> Result<Self> {
        let rust = wasm_bindgen::memory().unchecked_into::<WebAssembly::Memory>();
        let memory_types = MemoryTypes {
            rust_shared: rust.buffer().is_instance_of::<SharedArrayBuffer>(),
            imported_shared: shared,
        };
        thread_local! {
            static MODULES: RefCell<HashMap<MemoryTypes, WebAssembly::Module>> = RefCell::default();
        }
        let cached = MODULES.with(|modules| modules.borrow().get(&memory_types).cloned());
        let module = match cached {
            Some(module) => module,
            None => {
                let bytes = Uint8Array::from(encode(memory_types).as_slice());
                let module = WebAssembly::Module::new(&bytes)
                    .map_err(|error| js_error("compile memory transfers", error))?;
                MODULES.with(|modules| modules.borrow_mut().insert(memory_types, module.clone()));
                module
            }
        };
        let env = Object::new();
        set(&env, "rust", &rust)?;
        set(&env, "memory", memory)?;
        let imports = Object::new();
        set(&imports, "env", &env)?;
        let instance = WebAssembly::Instance::new(&module, &imports)
            .map_err(|error| js_error("link memory transfers", error))?;
        macro_rules! entry {
            ($name:literal) => {
                Binding::from_export(&instance, $name)
                    .map_err(|error| Error::new("bind memory transfer", error))?
            };
        }
        Ok(Self {
            read: entry!("read"),
            write: entry!("write"),
            read_u8: entry!("read_u8"),
            write_u8: entry!("write_u8"),
            read_u16: entry!("read_u16"),
            write_u16: entry!("write_u16"),
            read_u32: entry!("read_u32"),
            write_u32: entry!("write_u32"),
            read_u64: entry!("read_u64"),
            write_u64: entry!("write_u64"),
        })
    }

    pub(super) fn read(&self, offset: u32, bytes: &mut [u8]) {
        // The live slice supplies an allocated Rust destination for this call.
        self.read
            .call((bytes.as_mut_ptr() as i32, offset as i32, bytes.len() as i32));
    }

    pub(super) fn write(&self, offset: u32, bytes: &[u8]) {
        self.write
            .call((offset as i32, bytes.as_ptr() as i32, bytes.len() as i32));
    }

    pub(super) fn load<V: MemoryValue>(&self, offset: u32) -> V {
        V::load(self, offset)
    }

    pub(super) fn store<V: MemoryValue>(&self, offset: u32, value: V) {
        V::store(self, offset, value);
    }
}

macro_rules! scalar {
    ($type:ty, $load:ident, $store:ident) => {
        impl crate::memory::sealed::MemoryValue for $type {
            #[inline]
            fn load(transfers: &Transfers, offset: u32) -> Self {
                transfers.$load.call((offset,))
            }

            #[inline]
            fn store(transfers: &Transfers, offset: u32, value: Self) {
                transfers.$store.call((offset, value));
            }
        }
    };
}

scalar!(u8, read_u8, write_u8);
scalar!(u16, read_u16, write_u16);
scalar!(u32, read_u32, write_u32);
scalar!(u64, read_u64, write_u64);
