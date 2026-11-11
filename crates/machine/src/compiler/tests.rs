use super::*;
use crate::{CpuConfig, Hook, MachineConfig};
use relocade_wasm_runtime::{WasmBackend, WasmRuntime};
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};
use std::{mem::offset_of, rc::Rc};

#[test]
fn rejected_assumptions_leave_existing_hooks_installed() {
    pollster::block_on(async {
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let map = PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap();
        let memory = Memory::new(MachineConfig::new(0x1000, map).unwrap()).unwrap();
        let config = CpuConfig::new(compiler.target(), CpuState::real_mode()).unwrap();
        let mut cpu = Cpu::new(memory, config, Default::default()).await.unwrap();
        let owner = Rc::new(());
        cpu.install([Hook::host(0, {
            let owner = owner.clone();
            async move || {
                let _owner = &owner;
            }
        })])
        .await
        .unwrap();
        let blocks = compiler
            .compile_blocks(&[BlockSnapshot {
                start_eip: 0,
                bytes: &[0x90],
                instruction_limit: 1,
            }])
            .unwrap();
        let code = CompiledCode {
            module: WasmRuntime::new()
                .unwrap()
                .compiler()
                .compile(&blocks.bytes)
                .await
                .unwrap(),
            description: Description {
                addresses: vec![0],
                blocks: blocks.entries,
                target: blocks.execution_target,
                state_assumptions: blocks.state_assumptions,
            },
        };
        let mut rejected = code.clone();
        // A stronger supplied condition fails although the execution target matches.
        rejected
            .description
            .state_assumptions
            .push(StateAssumption {
                offset: offset_of!(CpuState, registers.eax) as u32,
                bytes: 4,
                mask: u32::MAX.into(),
                expected: 1,
            });
        assert!(matches!(
            rejected.install(&mut cpu),
            Err(Error::CompilationAssumptions)
        ));
        assert_eq!(Rc::strong_count(&owner), 2);
        code.install(&mut cpu).unwrap();
        assert_eq!(Rc::strong_count(&owner), 1);
    });
}
