//! Admit fetch-valid snapshots and reuse blocks with matching compilation inputs.

#[cfg(test)]
mod tests;

use relocade_x86::{Compiler, CpuState, ExecutionProfile, ExecutionTarget};
use std::collections::HashMap;
use wasmparser::Validator;

use super::step::TestModule;

#[derive(Debug, Eq, Hash, PartialEq)]
struct BlockKey {
    start: u32,
    bytes: Vec<u8>,
    limit: u32,
    target: ExecutionTarget,
}

#[derive(Default)]
pub(crate) struct BlockModules {
    modules: HashMap<BlockKey, TestModule>,
    complete_handoffs: bool,
}

impl BlockModules {
    /// Completes fallback instructions when a fixture checks full instruction
    /// semantics rather than the precise interpreter handoff boundary.
    pub(crate) fn with_interpreter() -> Self {
        Self {
            complete_handoffs: true,
            ..Self::default()
        }
    }

    pub(crate) fn get(
        &mut self,
        cpu: &CpuState,
        bytes: &[u8],
        limit: u32,
        profile: impl Into<ExecutionProfile>,
    ) -> &TestModule {
        self.get_with_target(cpu, bytes, limit, Compiler::new(profile).target())
    }

    pub(crate) fn get_with_target(
        &mut self,
        cpu: &CpuState,
        bytes: &[u8],
        limit: u32,
        target: ExecutionTarget,
    ) -> &TestModule {
        let key = BlockKey {
            start: cpu.eip,
            bytes: bytes.to_vec(),
            limit,
            target,
        };
        // A matching cache key only permits reuse after validating the current
        // execution context. The fixture machine supplies the code and mappings.
        key.validate_fetches(cpu);
        let complete_handoffs = self.complete_handoffs;
        self.modules.entry(key).or_insert_with_key(|key| {
            let module = Compiler::new(target.profile)
                .with_cpu_model(target.cpu_model)
                .compile_block(key.start, bytes, limit)
                .unwrap_or_else(|error| panic!("compiling {key:?}: {error:?}"));
            assert_eq!(module.execution_target, Some(target));
            Validator::new()
                .validate_all(&module.bytes)
                .unwrap_or_else(|error| panic!("validating {key:?}: {error}"));
            let module = TestModule::new(&module);
            if complete_handoffs {
                module.with_interpreter(TestModule::interpreter_with_target(target))
            } else {
                module
            }
        })
    }
}

impl BlockKey {
    fn validate_fetches(&self, cpu: &CpuState) {
        assert!(self.target.is_compatible_with(self.target, &cpu.segments));
        let cs = cpu.segments.cs;
        assert!(cs.usable != 0 && cs.executable != 0);
        let mut eip = self.start;
        let mut remaining = self.bytes.as_slice();
        for _ in 0..self.limit {
            let (decoded, rest) =
                crate::decode::snapshot(remaining, eip, self.target.profile.code_default_size())
                    .unwrap_or_else(|error| panic!("validating snapshot {self:?}: {error:?}"));
            let last = decoded.fallthrough_eip.wrapping_sub(1);
            assert!(
                cs.limit == u32::MAX || (last >= eip && last <= cs.limit),
                "snapshot instruction {eip:#x}..={last:#x} exceeds CS limit {:#x}",
                cs.limit
            );
            if decoded.instruction.ends_block()
                || decoded.instruction.fails_requirements(self.target)
            {
                break;
            }
            eip = decoded.fallthrough_eip;
            remaining = rest;
        }
    }
}
