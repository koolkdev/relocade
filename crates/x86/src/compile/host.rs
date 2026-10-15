//! Host services compiled as ordinary callable execution entries.

use super::{dispatch::Dispatch, Compiler, EntryTable};
use crate::{
    decode, execution::ExecutionBuilder, instruction::DecodedInstruction, memory::Memory,
    runtime::Runtime, state::Cpu, CompiledModule,
};
use wasm86_compiler::{FunctionImport, Program, Signature, Type, I32};

/// Return semantics lowered through the same implementation as guest instructions.
/// Width follows the compiler's execution profile.
#[derive(Clone, Copy, Debug)]
pub enum HostReturn {
    Interrupt,
    Near { pop_bytes: u16 },
    Far { pop_bytes: u16 },
}

/// A host service at a linear execution address. Its import has no parameters:
/// the binding reads and publishes CPU/memory through the ordinary execution ABI.
#[derive(Clone, Copy, Debug)]
pub struct HostFunction {
    pub address: u32,
    pub return_: HostReturn,
}

/// One execution-address mapping produced while compiling a module.
#[derive(Clone, Debug)]
pub struct CallableEntry {
    pub address: u32,
    pub export: String,
}

/// A single Wasm module with an entry and a resumable return function per service.
/// Install `exports` in consecutive slots beginning at the compilation's first
/// slot, then publish `entries`. Imports are `host.entry_0`, `host.entry_1`, etc.
pub struct CompiledHostFunctions {
    pub module: CompiledModule,
    pub exports: Vec<String>,
    pub entries: Vec<CallableEntry>,
}

impl Compiler {
    /// Generates direct host calls and lowered returns, with no guest-byte fetch
    /// and no runtime host-ID lookup. When budgeted, each callback costs one work
    /// unit; its return costs one more and retires normally. A yield or return
    /// fault resumes at the compiled return, preserving completed host effects.
    /// CPU observations never specialize these functions.
    pub fn compile_host_functions(
        &self,
        functions: &[HostFunction],
        first_slot: u32,
    ) -> Result<CompiledHostFunctions, wasm86_compiler::BuildError> {
        assert!(first_slot >= EntryTable::FIRST_DYNAMIC_SLOT);
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let memory = Memory::declare(&mut program, self.target.profile)?;
        let runtime = Runtime::declare(&mut program, self.execution_budget);
        let dispatch = Dispatch::declare(&mut program);
        let mut exports = Vec::new();
        let mut entries = Vec::new();
        for (index, function) in functions.iter().enumerate() {
            let slot = first_slot
                .checked_add(u32::try_from(index * 2).expect("function count fits table"))
                .expect("function slots fit table");
            let entry_name = format!("entry_{index}");
            let resume_name = format!("resume_{index}");
            let import = program.import_function(FunctionImport {
                module: "host".into(),
                name: entry_name.clone(),
                signature: Signature {
                    parameters: vec![],
                    results: vec![Type::I32],
                },
            });
            let entry = program.function(
                Signature {
                    parameters: vec![],
                    results: vec![Type::I64],
                },
                |mut body| {
                    runtime.check_entry_budget(&mut body)?;
                    if let Some(work) = runtime.remaining_work(&mut body)? {
                        runtime.publish_work(&mut body, Some(&work.sub(1)))?;
                    }
                    dispatch.suspend(&mut body, &cpu, slot + 1)?;
                    let action = body.call::<I32>(import, &[])?;
                    body.if_(action.eq(1), |yielding| {
                        yielding.return_(crate::HOST_YIELD | u64::from(function.address))
                    })?;
                    body.if_(action.ne(0), |invalid| invalid.trap())?;
                    dispatch.enter(body, EntryTable::DISPATCH_SLOT)
                },
            )?;
            let encoding = match function.return_ {
                HostReturn::Interrupt => vec![0xcf],
                HostReturn::Near { pop_bytes: 0 } => vec![0xc3],
                HostReturn::Far { pop_bytes: 0 } => vec![0xcb],
                HostReturn::Near { pop_bytes } => {
                    vec![0xc2, pop_bytes as u8, (pop_bytes >> 8) as u8]
                }
                HostReturn::Far { pop_bytes } => {
                    vec![0xca, pop_bytes as u8, (pop_bytes >> 8) as u8]
                }
            };
            let (decoded, _) =
                decode::snapshot(&encoding, 0, self.target.profile.code_default_size())
                    .expect("fixed return encoding is supported");
            let resume = program.function(
                Signature {
                    parameters: vec![],
                    results: vec![Type::I64],
                },
                |mut body| {
                    runtime.check_entry_budget(&mut body)?;
                    let eip = cpu.read_eip(&mut body)?;
                    let mut execution = ExecutionBuilder::new(
                        body,
                        &cpu,
                        Some(&memory),
                        runtime,
                        &eip,
                        self.target,
                    )?;
                    execution.execute(DecodedInstruction {
                        instruction: decoded.instruction,
                        eip: eip.clone(),
                        fallthrough_eip: eip,
                    })?;
                    execution.complete(|body, eip| runtime.dispatch(body, eip))
                },
            )?;
            program.export(&entry_name, entry)?;
            program.export(&resume_name, resume)?;
            entries.push(CallableEntry {
                address: function.address,
                export: entry_name.clone(),
            });
            exports.extend([entry_name, resume_name]);
        }
        Ok(CompiledHostFunctions {
            module: CompiledModule {
                bytes: program.compile()?,
                entry: exports.first().cloned().unwrap_or_default(),
                execution_target: Some(self.target),
                execution_budget: self.execution_budget,
            },
            exports,
            entries,
        })
    }
}
