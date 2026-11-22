//! Host imports supplied by the execution environment of this CPU.

use super::{segments, IoBus, Result, RuntimeMemory, WasmRuntime};
use relocade_wasm_runtime::Import;
use relocade_x86::DescriptorTables;
use std::sync::{Arc, Mutex, MutexGuard};

pub(in crate::runtime) enum Environment {
    Pc(IoBus),
    Flat {
        tables: Arc<Mutex<DescriptorTables>>,
        imports: [Import<'static, WasmRuntime>; 3],
    },
}

impl Environment {
    pub(super) fn new(
        wasm: &mut WasmRuntime,
        io: Option<IoBus>,
        bindings: &RuntimeMemory,
        tables: Option<Arc<Mutex<DescriptorTables>>>,
    ) -> Result<Self> {
        let Some(io) = io else {
            let tables = tables.unwrap_or_else(|| {
                bindings
                    .guest
                    .shared_descriptors()
                    .expect("flat memory owns descriptor tables")
                    .clone()
            });
            let imports = segments::imports(wasm, tables.clone());
            return Ok(Self::Flat { tables, imports });
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
            Self::Flat { .. } => panic!("this CPU has no PC device bus"),
        }
    }

    pub(in crate::runtime) fn descriptor_tables(&self) -> Option<MutexGuard<'_, DescriptorTables>> {
        match self {
            Self::Pc(_) => None,
            Self::Flat { tables, .. } => Some(tables.lock().unwrap()),
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
            Self::Flat {
                imports: descriptors,
                ..
            } => imports.extend(descriptors.iter().cloned()),
        }
    }
}
