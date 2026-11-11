//! The resources borrowed by guest execution and its host services.

use super::{CpuContext, Dispatch, RuntimeMemory, WasmRuntime};
use crate::IoBus;
use relocade_x86::ExecutionExit;

pub(super) struct CpuInstance {
    pub wasm: WasmRuntime,
    pub bindings: RuntimeMemory,
    pub io: IoBus,
    pub dispatch: Dispatch,
}

impl CpuInstance {
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.bindings.cpu,
            memory: self.bindings.guest.view().with_io(&self.io),
            io: &self.io,
            guest_calls: None,
        }
    }

    pub fn dispatch(&mut self) -> ExecutionExit {
        self.dispatch.run(&mut self.wasm)
    }
}
