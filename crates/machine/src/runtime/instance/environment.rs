//! Host imports supplied by the execution environment of this CPU.

use super::{segments, IoBus, Result, RuntimeMemory, WasmRuntime};
use relocade_wasm_runtime::Import;

pub(in crate::runtime) enum Environment {
    Pc(IoBus),
    Flat([Import<'static, WasmRuntime>; 3]),
}

impl Environment {
    pub(super) fn new(
        wasm: &mut WasmRuntime,
        io: Option<IoBus>,
        bindings: &RuntimeMemory,
    ) -> Result<Self> {
        let Some(io) = io else {
            return Ok(Self::Flat(segments::imports(wasm)));
        };
        let bus = io.downgrade();
        io.port_map().initialize(wasm, &bus.0)?;
        io.mmio_map().initialize(
            wasm,
            &bus.0,
            &bindings.mapping,
            bindings.guest.configuration().memory_map(),
        )?;
        Ok(Self::Pc(io))
    }

    pub(in crate::runtime) fn pc_bus(&self) -> &IoBus {
        match self {
            Self::Pc(io) => io,
            Self::Flat(_) => panic!("this CPU has no PC device bus"),
        }
    }

    pub(super) fn extend_imports<'a>(
        &'a self,
        wasm: &WasmRuntime,
        imports: &mut Vec<Import<'a, WasmRuntime>>,
    ) {
        match self {
            Self::Pc(io) => {
                imports.extend(io.port_map().imports(wasm));
                imports.extend(io.mmio_map().imports(wasm));
            }
            Self::Flat(segments) => imports.extend(segments.iter().cloned()),
        }
    }
}
