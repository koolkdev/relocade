//! Admit fetch-valid snapshots and reuse blocks with matching compilation inputs.

#[cfg(test)]
mod tests;

use relocade_x86::{Compiler, CompilerConfig, CpuState, InstructionFlow};
use std::collections::HashMap;
use wasmparser::Validator;

use super::step::TestModule;

#[derive(Debug, Eq, Hash, PartialEq)]
struct BlockKey {
    start: u32,
    bytes: Vec<u8>,
    limit: u32,
    config: CompilerConfig,
}

#[derive(Default)]
pub(crate) struct BlockModules(HashMap<BlockKey, TestModule>);

impl BlockModules {
    pub(crate) fn get(
        &mut self,
        cpu: &CpuState,
        bytes: &[u8],
        limit: u32,
        config: CompilerConfig,
    ) -> &TestModule {
        let key = BlockKey {
            start: cpu.eip,
            bytes: bytes.to_vec(),
            limit,
            config,
        };
        // A matching cache key only permits reuse after validating the current
        // execution context. The fixture machine supplies the code and mappings.
        key.validate_fetches(cpu);
        self.0.entry(key).or_insert_with_key(|key| {
            let module = Compiler::from_config(config)
                .compile_block(key.start, bytes, limit)
                .unwrap_or_else(|error| panic!("compiling {key:?}: {error:?}"));
            assert_eq!(module.config, Some(config));
            Validator::new()
                .validate_all(&module.bytes)
                .unwrap_or_else(|error| panic!("validating {key:?}: {error}"));
            TestModule::new(&module)
        })
    }
}

impl BlockKey {
    fn validate_fetches(&self, cpu: &CpuState) {
        assert!(self.config.is_compatible_with(self.config, cpu));
        let cs = cpu.segments.cs;
        assert!(cs.usable != 0 && cs.executable != 0);
        let mut eip = self.start;
        let mut remaining = self.bytes.as_slice();
        for _ in 0..self.limit {
            let (decoded, rest) = crate::decode::snapshot(
                remaining,
                eip,
                self.config
                    .entry
                    .code_default_size
                    .expect("fixture has a fixed code width"),
            )
            .unwrap_or_else(|error| panic!("validating snapshot {self:?}: {error:?}"));
            let last = decoded.next_eip().wrapping_sub(1);
            assert!(
                cs.limit == u32::MAX || (last >= eip && last <= cs.limit),
                "snapshot instruction {eip:#x}..={last:#x} exceeds CS limit {:#x}",
                cs.limit
            );
            if decoded.flow(self.config) != InstructionFlow::Continue {
                break;
            }
            eip = decoded.next_eip();
            remaining = rest;
        }
    }
}

/// Declared extent of a straight-line test block, including its terminating instruction.
pub(crate) fn instruction_count(code: &[u8], limit: usize, config: CompilerConfig) -> usize {
    let mut bytes = code;
    let mut eip = 0;
    for index in 0..limit {
        let (decoded, rest) = crate::decode::snapshot(
            bytes,
            eip,
            config
                .entry
                .code_default_size
                .expect("fixture has a fixed code width"),
        )
        .expect("fixture instructions decode");
        if decoded.flow(config) != InstructionFlow::Continue {
            return index + 1;
        }
        bytes = rest;
        eip = decoded.next_eip();
    }
    limit
}
