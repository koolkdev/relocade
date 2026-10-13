use crate::{memory, memory::MemoryAccess, set_property};
use js_sys::{Object, WebAssembly::Memory};
use std::{cell::RefCell, rc::Rc};
use wasm86_runtime::{backend::Imports, Devices};
use wasm86_x86::CpuState;
use wasm_bindgen::closure::Closure;

/// Retains the Rust closures for as long as the Wasm instance can call them.
pub(crate) struct BoundImports {
    pub object: Object,
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
    pub fn bind_dispatcher(&self, exports: &Object) {
        use wasm_bindgen::JsCast;
        let namespace = js_sys::Reflect::get(&self.object, &"wasm86".into())
            .unwrap()
            .unchecked_into::<Object>();
        for name in ["dispatch", "interpret"] {
            let function = js_sys::Reflect::get(exports, &name.into()).unwrap();
            set_property(&namespace, name, &function);
        }
    }

    pub fn new<D: Devices + 'static>(
        state: &Rc<RefCell<Imports<D>>>,
        cpu: &Memory,
        budget: &Memory,
        entries: &Memory,
        memory: &MemoryAccess,
        functions: &js_sys::WebAssembly::Table,
    ) -> Self {
        let callback = || CallbackAccess {
            state: state.clone(),
            memory: memory.clone(),
        };
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
        set_property(&namespace, "executionBudget", budget);
        set_property(&namespace, "executionEntries", entries);
        set_property(&namespace, "executionFunctions", functions);
        set_property(&namespace, "guest", &memory.guest);
        set_property(&namespace, "physicalMap", &memory.physical);
        set_property(&namespace, "readPort", read_port.as_ref());
        set_property(&namespace, "writePort", write_port.as_ref());
        set_property(&namespace, "readMmio", read_mmio.as_ref());
        set_property(&namespace, "writeMmio", write_mmio.as_ref());
        let object = Object::new();
        set_property(&object, "wasm86", &namespace);
        Self {
            object,
            _read_port: read_port,
            _write_port: write_port,
            _read_mmio: read_mmio,
            _write_mmio: write_mmio,
        }
    }
}

/// A module's direct host imports, retained beside its installed Wasm functions.
pub(crate) struct HostImports {
    pub object: Object,
    _callbacks: Vec<Closure<dyn FnMut() -> u32>>,
}

impl HostImports {
    pub fn new<D: Devices + 'static>(
        common: &Object,
        state: &Rc<RefCell<Imports<D>>>,
        cpu: &Memory,
        memory: &MemoryAccess,
        callbacks: Vec<wasm86_runtime::backend::HostCallback<D>>,
    ) -> Self {
        let namespace = Object::new();
        let mut closures = Vec::new();
        for (index, callback) in callbacks.into_iter().enumerate() {
            let binding = state.borrow_mut().bind_host_call(callback);
            let mut access = CallbackAccess {
                state: state.clone(),
                memory: memory.clone(),
            };
            let cpu = cpu.clone();
            let closure = Closure::new(move || {
                let mut bytes = [0; CpuState::BYTE_LEN];
                memory::read(&cpu, 0, &mut bytes);
                let mut state = CpuState::from_bytes(bytes);
                let action =
                    access.access(|imports, memory| imports.host_call(&mut state, memory, binding));
                memory::write(&cpu, 0, &state.to_bytes());
                action
            });
            set_property(&namespace, &format!("entry_{index}"), closure.as_ref());
            closures.push(closure);
        }
        let object = Object::assign(&Object::new(), common);
        set_property(&object, "hooks", &namespace);
        Self {
            object,
            _callbacks: closures,
        }
    }
}
