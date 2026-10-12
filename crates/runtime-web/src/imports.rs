use crate::{memory::MemoryAccess, set_property};
use js_sys::{Object, WebAssembly::Memory};
use std::{cell::RefCell, rc::Rc};
use wasm86_runtime::{backend::Imports, Devices};
use wasm_bindgen::closure::Closure;

/// Retains the Rust closures for as long as the Wasm instance can call them.
pub(crate) struct BoundImports {
    pub object: Object,
    _dispatch: Closure<dyn FnMut(u32) -> i64>,
    _read_port: Closure<dyn FnMut(u32, u32) -> u32>,
    _write_port: Closure<dyn FnMut(u32, u32, u32)>,
    _read_mmio: Closure<dyn FnMut(u32, u32) -> u64>,
    _write_mmio: Closure<dyn FnMut(u32, u32, u64)>,
}

struct CallbackAccess<D> {
    state: Rc<RefCell<Imports<D>>>,
    memory: MemoryAccess,
}

impl<D: Devices> CallbackAccess<D> {
    fn access<R>(&mut self, access: impl FnOnce(&mut Imports<D>, &mut MemoryAccess) -> R) -> R {
        access(&mut self.state.borrow_mut(), &mut self.memory)
    }
}

impl BoundImports {
    pub fn new<D: Devices + 'static>(
        state: &Rc<RefCell<Imports<D>>>,
        cpu: &Memory,
        memory: &MemoryAccess,
    ) -> Self {
        let callback = || CallbackAccess {
            state: state.clone(),
            memory: memory.clone(),
        };
        let dispatch_state = state.clone();
        let dispatch = Closure::new(move |_: u32| dispatch_state.borrow_mut().dispatch());
        let mut read_port_access = callback();
        let read_port = Closure::new(move |port: u32, bytes: u32| {
            read_port_access
                .access(|imports, memory| imports.read_port(memory, port as u16, bytes as u8))
        });
        let mut write_port_access = callback();
        let write_port = Closure::new(move |port: u32, bytes: u32, value: u32| {
            write_port_access.access(|imports, memory| {
                imports.write_port(memory, port as u16, bytes as u8, value)
            })
        });
        let mut read_mmio_access = callback();
        let read_mmio = Closure::new(move |address: u32, bytes: u32| {
            read_mmio_access
                .access(|imports, memory| imports.read_mmio(memory, address, bytes as u8))
        });
        let mut write_mmio_access = callback();
        let write_mmio = Closure::new(move |address: u32, bytes: u32, value: u64| {
            write_mmio_access
                .access(|imports, memory| imports.write_mmio(memory, address, bytes as u8, value))
        });
        let namespace = Object::new();
        set_property(&namespace, "cpuState", cpu);
        set_property(&namespace, "guest", &memory.guest);
        set_property(&namespace, "physicalMap", &memory.physical);
        set_property(&namespace, "dispatch", dispatch.as_ref());
        set_property(&namespace, "readPort", read_port.as_ref());
        set_property(&namespace, "writePort", write_port.as_ref());
        set_property(&namespace, "readMmio", read_mmio.as_ref());
        set_property(&namespace, "writeMmio", write_mmio.as_ref());
        let object = Object::new();
        set_property(&object, "wasm86", &namespace);
        Self {
            object,
            _dispatch: dispatch,
            _read_port: read_port,
            _write_port: write_port,
            _read_mmio: read_mmio,
            _write_mmio: write_mmio,
        }
    }
}
