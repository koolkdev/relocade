#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use crate::{Compiler, ExecutionProfile, ExecutionTarget, SegmentProfile};
use relocade_test_support::Module;
pub(crate) use relocade_test_support::{Outcome, Value as Argument};

mod segments;
mod wasmtime;
pub(crate) use segments::{SegmentQuery, SegmentResolution};

pub(crate) struct Input {
    pub(crate) cpu: Vec<u8>,
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
    pub(crate) physical_pages: Vec<(u32, u32, bool)>,
    pub(crate) a20_enabled: bool,
    pub(crate) mmio_pages: Vec<(u32, u32)>,
    pub(crate) mmio_updates: Vec<MmioUpdate>,
    pub(crate) observe_mmio: bool,
    pub(crate) port_reads: Vec<u32>,
    pub(crate) arguments: Vec<Argument>,
    pub(crate) observe_guest: bool,
    pub(crate) segment_resolutions: Vec<SegmentResolution>,
    pub(crate) segment_queries: Vec<SegmentQuery>,
    pub(crate) patches_before_calls: Vec<CallPatches>,
    pub(crate) dispatch_return: i64,
}

impl Input {
    pub(crate) fn new(cpu: &[u8]) -> Self {
        Self {
            cpu: cpu.to_vec(),
            guest: Vec::new(),
            machine: Vec::new(),
            physical_pages: Vec::new(),
            // Most execution fixtures exercise unwrapped physical addressing.
            a20_enabled: true,
            mmio_pages: Vec::new(),
            mmio_updates: Vec::new(),
            observe_mmio: false,
            port_reads: Vec::new(),
            arguments: Vec::new(),
            observe_guest: false,
            segment_resolutions: Vec::new(),
            segment_queries: Vec::new(),
            patches_before_calls: Vec::new(),
            dispatch_return: i64::MIN,
        }
    }
}

/// Device changes applied after the corresponding MMIO transfer completes.
#[derive(Clone, Default)]
pub(crate) struct MmioUpdate {
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) map: Vec<(u32, Vec<u8>)>,
}

/// Host memory edits applied immediately before an entry invocation.
#[derive(Default)]
pub(crate) struct CallPatches {
    pub(crate) cpu: Vec<(u32, Vec<u8>)>,
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
}

impl CallPatches {
    pub(crate) fn cpu(patches: Vec<(u32, Vec<u8>)>) -> Self {
        Self {
            cpu: patches,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) cpu: Vec<u8>,
    pub(crate) guest: Option<Vec<(u32, u8)>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    PortRead {
        port: u32,
        bytes: u32,
    },
    PortWrite {
        port: u32,
        bytes: u32,
        value: u32,
    },
    MmioRead {
        address: u32,
        bytes: u32,
    },
    MmioWrite {
        address: u32,
        value: Vec<u8>,
    },
    Interpret {
        snapshot: Snapshot,
    },
    QuerySegmentDescriptor {
        selector: i32,
    },
    ResolveSegment {
        segment: i32,
        selector: i32,
    },
    Dispatch {
        snapshot: Snapshot,
    },
    Return {
        outcome: Outcome,
        snapshot: Snapshot,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Observation {
    pub(crate) events: Vec<Event>,
    pub(crate) guest_unchanged: bool,
    /// True only if machine memory is unchanged at every observed boundary.
    pub(crate) machine_unchanged: bool,
}

pub(crate) struct TestModule {
    module: Module,
    pub(crate) entry: String,
    target: Option<ExecutionTarget>,
    interpreter: Option<&'static TestModule>,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum InterpreterEntry {
    Step,
    Run,
}

impl TestModule {
    pub(crate) fn new(module: &crate::CompiledModule) -> Self {
        Self {
            module: Module::new(&module.bytes),
            entry: module.entry.clone(),
            target: module.execution_target,
            interpreter: None,
        }
    }

    pub(crate) fn interpreter() -> &'static Self {
        Self::interpreter_with_profile(ExecutionProfile::Protected(SegmentProfile::Flat32))
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.module.bytes()
    }

    pub(crate) fn interpreter_with_profile(profile: impl Into<ExecutionProfile>) -> &'static Self {
        Self::interpreter_with_target(Compiler::new(profile).target())
    }

    pub(crate) fn interpreter_with_target(target: ExecutionTarget) -> &'static Self {
        Self::cached_interpreter(target, InterpreterEntry::Step)
    }

    pub(crate) fn interpreter_run_with_profile(
        profile: impl Into<ExecutionProfile>,
    ) -> &'static Self {
        Self::interpreter_run_with_target(Compiler::new(profile).target())
    }

    pub(crate) fn interpreter_run_with_target(target: ExecutionTarget) -> &'static Self {
        Self::cached_interpreter(target, InterpreterEntry::Run)
    }

    fn cached_interpreter(target: ExecutionTarget, entry: InterpreterEntry) -> &'static Self {
        static MODULES: OnceLock<
            Mutex<HashMap<(ExecutionTarget, InterpreterEntry), &'static OnceLock<TestModule>>>,
        > = OnceLock::new();
        let cell = *MODULES
            .get_or_init(Mutex::default)
            .lock()
            .unwrap()
            .entry((target, entry))
            .or_insert_with(|| Box::leak(Box::new(OnceLock::new())));
        cell.get_or_init(|| {
            let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
            let module = match entry {
                InterpreterEntry::Step => compiler.compile_interpreter_step(),
                InterpreterEntry::Run => compiler.compile_interpreter(),
            };
            Self::new(&module.unwrap())
        })
    }

    /// Link the interpreter directly; unlinked fixtures stop at the handoff callback.
    pub(crate) fn with_interpreter(mut self, interpreter: &'static Self) -> Self {
        assert_eq!(self.target, interpreter.target);
        self.interpreter = Some(interpreter);
        self
    }

    fn check_profile(&self, bytes: &[u8]) {
        let Some(target) = self.target else {
            return;
        };
        let cpu = crate::CpuState::from_bytes(
            bytes[..crate::CpuState::BYTE_LEN]
                .try_into()
                .expect("the CPU image"),
        );
        assert!(
            target.is_compatible_with(target, &cpu.segments),
            "{} requires compatible {target:?} segment state",
            self.entry
        );
    }
}

fn changes(before: &[u8], after: &[u8]) -> Vec<(u32, u8)> {
    before
        .iter()
        .zip(after)
        .enumerate()
        .filter(|(_, (old, new))| old != new)
        .map(|(offset, (_, &new))| (offset as u32, new))
        .collect()
}
