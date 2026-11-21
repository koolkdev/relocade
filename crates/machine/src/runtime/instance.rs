//! Owns bound Wasm execution, module installation and resources for one CPU.

mod code_cache;

use super::{code, CpuContext, RuntimeMemory, WasmRuntime, WasmRuntimeHandle};
use crate::{memory::CpuBinding, FarPointer, IoBus, Result};
pub(crate) use code_cache::CodeWake;
pub(super) use code_cache::{CodeCache, Installation};
use relocade_wasm_runtime::{Function, SharedMemory, WasmBackend};
use relocade_x86::{abi, ExecutionExit};
use std::rc::Rc;

/// An ordinary `() -> ExecutionExit` export at a packed real-mode CS:IP key.
pub struct DispatchEntry<'a> {
    pub key: u32,
    pub export: &'a str,
}

/// Wasm exports through which the host can enter the guest.
struct GuestEntryPoints {
    dispatch: Function<WasmRuntime, abi::ExecutionSignature>,
    step: Function<WasmRuntime, abi::ExecutionSignature>,
    interrupt: Function<WasmRuntime, abi::InterruptSignature>,
    far_call: Function<WasmRuntime, abi::FarCallSignature>,
}

/// Binds guest execution and installed modules to one runtime and its memory.
/// Runtime borrows stay inside synchronous operations; compilation and host
/// service polling do not hold the runtime.
pub(super) struct CpuInstance {
    // Retire registrations before releasing the guest memory attachment.
    pub(super) code: CodeCache,
    pub(super) guest: Rc<GuestRuntime>,
}

/// Runtime resources imported by modules and used to enter this guest.
pub(super) struct GuestRuntime {
    runtime: WasmRuntimeHandle,
    pub(super) bindings: RuntimeMemory,
    pub(super) io: IoBus,
    entry_points: GuestEntryPoints,
    interpret: Function<WasmRuntime, abi::ExecutionSignature>,
    resolve_call: Function<WasmRuntime, fn() -> (u32, bool)>,
    resolve_and_jump: Function<WasmRuntime, abi::ExecutionSignature>,
}

impl CpuInstance {
    pub(super) async fn new(binding: CpuBinding, io: IoBus, storage: SharedMemory) -> Result<Self> {
        let runtime = io.attach_cpu()?;
        let bindings = runtime.with_runtime(|wasm| RuntimeMemory::new(wasm, binding, storage))?;
        let target = bindings.configuration().execution_target();
        let (compiler, cache) =
            runtime.with_runtime(|wasm| (wasm.compiler(), wasm.module_cache().cloned()));
        let dispatcher = code::dispatcher(&compiler, target).await?;
        let interpreter = code::pc_interpreter(&compiler, cache.as_ref()).await?;
        runtime.with_runtime(|wasm| {
            // Link the dispatcher first; the interpreter imports its entry points.
            let mut table = code_cache::DispatchTable::new(wasm)?;
            let write_code = code_cache::WriteCode::new(wasm, bindings.guest.clone());
            let call_profile = wasm.create_memory(abi::CALL_PROFILE)?;
            let mut imports = bindings.imports(wasm).to_vec();
            imports.push(write_code.import());
            imports.extend(table.imports(wasm));
            imports.push(wasm.bind(abi::CALL_PROFILE, &call_profile));
            let instance = wasm.instantiate(&dispatcher, &imports)?;
            let dispatch = wasm.function(&instance, abi::DISPATCH)?;
            let interpret = wasm.function(&instance, abi::INTERPRET)?;
            let resolve_call = wasm.function(&instance, abi::RESOLVE_CALL)?;
            let resolve_and_jump = wasm.function(&instance, abi::RESOLVE_AND_JUMP)?;

            imports.extend([
                wasm.bind(abi::DISPATCH, &dispatch),
                wasm.bind(abi::INTERPRET, &interpret),
                wasm.bind(abi::RESOLVE_CALL, &resolve_call),
            ]);
            io.port_map().initialize(wasm, &io.downgrade().0)?;
            imports.extend(io.port_map().imports(wasm));
            io.mmio_map().initialize(
                wasm,
                &io.downgrade().0,
                &bindings.mapping,
                bindings.guest.configuration().memory_map(),
            )?;
            imports.extend(io.mmio_map().imports(wasm));
            let instance = wasm.instantiate(&interpreter, &imports)?;
            table.install_interpreter(wasm, &instance)?;
            let step = wasm.function(&instance, abi::STEP)?;
            let interrupt = wasm.function(&instance, abi::INTERRUPT)?;
            let far_call = wasm.function(&instance, abi::FAR_CALL)?;
            let guest = Rc::new(GuestRuntime {
                runtime: runtime.clone(),
                bindings,
                io,
                entry_points: GuestEntryPoints {
                    dispatch,
                    step,
                    interrupt,
                    far_call,
                },
                interpret,
                resolve_call,
                resolve_and_jump,
            });
            Ok(Self {
                code: CodeCache::new(guest.clone(), table, write_code, call_profile),
                guest,
            })
        })
    }

    pub(super) fn context(&mut self) -> CpuContext<'_> {
        CpuContext {
            cpu: &self.guest.bindings.cpu,
            memory: self.guest.bindings.guest.view().with_io(&self.guest.io),
            io: &self.guest.io,
            guest_calls: None,
        }
    }

    /// Retire code patched by a stopped host before the guest can follow a cached link.
    fn enter_guest(
        &mut self,
        enter: impl FnOnce(&GuestEntryPoints, &mut WasmRuntime) -> ExecutionExit,
    ) -> ExecutionExit {
        self.code.retire_pending().expect("retire modified code");
        let _write_callback = self.code.bind_write_callback();
        self.guest
            .runtime
            .with_runtime(|wasm| enter(&self.guest.entry_points, wasm))
    }

    pub(super) fn run(&mut self) -> ExecutionExit {
        loop {
            let exit = self.enter_guest(|entries, wasm| entries.dispatch.call(wasm, ()));
            if exit != ExecutionExit::CallProfileFull {
                return exit;
            }
            self.code.collect_call_profile();
        }
    }

    pub(super) fn enter_interrupt(&mut self, vector: u8) -> ExecutionExit {
        self.enter_guest(|entries, wasm| entries.interrupt.call(wasm, (vector,)))
    }

    pub(super) fn enter_far_call(&mut self, target: FarPointer, stack_bytes: u16) -> ExecutionExit {
        self.enter_guest(|entries, wasm| {
            entries
                .far_call
                .call(wasm, (target.segment, target.offset, stack_bytes))
        })
    }

    #[cold]
    pub(super) fn step(&mut self) -> ExecutionExit {
        self.enter_guest(|entries, wasm| entries.step.call(wasm, ()))
    }
}
