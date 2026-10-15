use crate::{memory::MemoryAccess, set_property};
use js_sys::{Array, Object, WebAssembly::Memory};
use std::{cell::RefCell, rc::Rc};
use wasm86_machine::Environment;
use wasm86_runtime::backend::Imports;
use wasm_bindgen::closure::Closure;

/// Retains the Rust closures for as long as the Wasm instance can call them.
pub(crate) struct BoundImports {
    pub object: Object,
    _resolve_segment: Closure<dyn FnMut(u32, u32) -> Array>,
    _query_descriptor: Closure<dyn FnMut(u32) -> Array>,
    _read_port: Closure<dyn FnMut(u32, u32) -> u32>,
    _write_port: Closure<dyn FnMut(u32, u32, u32)>,
    _read_mmio: Closure<dyn FnMut(u32, u32) -> u64>,
    _write_mmio: Closure<dyn FnMut(u32, u32, u64)>,
}

struct CallbackAccess<D> {
    state: Rc<RefCell<Imports<D>>>,
    memory: MemoryAccess,
}

impl<D: Environment> CallbackAccess<D> {
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

    pub fn new<D: Environment + 'static>(
        state: &Rc<RefCell<Imports<D>>>,
        context: &Memory,
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
        let descriptor_state = state.clone();
        let resolve_segment = Closure::new(move |segment: u32, selector: u32| {
            let (status, error, base, limit, selector, attributes) = descriptor_state
                .borrow_mut()
                .resolve_segment(segment, selector);
            [status, error, base, limit, selector, attributes]
                .into_iter()
                .map(wasm_bindgen::JsValue::from)
                .collect::<Array>()
        });
        let descriptor_state = state.clone();
        let query_descriptor = Closure::new(move |selector: u32| {
            let (visible, readable, writable, rights, limit) = descriptor_state
                .borrow_mut()
                .query_segment_descriptor(selector);
            [visible, readable, writable, rights, limit]
                .into_iter()
                .map(wasm_bindgen::JsValue::from)
                .collect::<Array>()
        });
        let namespace = Object::new();
        set_property(&namespace, "executionContext", context);
        set_property(&namespace, "executionEntries", entries);
        set_property(&namespace, "executionFunctions", functions);
        set_property(&namespace, "guest", &memory.guest);
        set_property(
            &namespace,
            state.borrow().memory_map().import_name(),
            &memory.mapping,
        );
        set_property(&namespace, "readPort", read_port.as_ref());
        set_property(&namespace, "writePort", write_port.as_ref());
        set_property(&namespace, "readMmio", read_mmio.as_ref());
        set_property(&namespace, "writeMmio", write_mmio.as_ref());
        set_property(&namespace, "resolveSegment", resolve_segment.as_ref());
        set_property(
            &namespace,
            "querySegmentDescriptor",
            query_descriptor.as_ref(),
        );
        let object = Object::new();
        set_property(&object, "wasm86", &namespace);
        Self {
            object,
            _resolve_segment: resolve_segment,
            _query_descriptor: query_descriptor,
            _read_port: read_port,
            _write_port: write_port,
            _read_mmio: read_mmio,
            _write_mmio: write_mmio,
        }
    }
}
