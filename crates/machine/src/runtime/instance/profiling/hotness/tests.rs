use super::*;
use crate::{Cpu, CpuConfig, DispatchEntry, Hook, MachineConfig, Memory};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};

#[test]
fn ordinary_batches_wait_for_sixty_four_eligible_functions() {
    let mut hotness = Hotness::default();
    hotness.record(CallCount {
        target: 1,
        count: 1023,
    });
    assert!(hotness.take_batch(|_| false).is_empty());
    hotness.record(CallCount {
        target: 1,
        count: 1,
    });
    assert!(hotness.take_batch(|_| false).is_empty());
    for target in 2..=63 {
        hotness.record(CallCount {
            target,
            count: 1024,
        });
    }
    assert!(hotness.take_batch(|_| false).is_empty());
    hotness.record(CallCount {
        target: 64,
        count: 1024,
    });
    assert_eq!(hotness.take_batch(|_| false), (1..=64).collect::<Vec<_>>());
    hotness.record(CallCount {
        target: 2,
        count: 10000,
    });
    assert!(hotness.take_batch(|_| false).is_empty());
    assert_eq!(hotness.count(2), 11024);
}

#[test]
fn a_hot_function_forces_a_small_batch_with_other_eligible_candidates() {
    let mut hotness = Hotness::default();
    for (target, count) in [(1, 4095), (2, 1024), (3, 1023)] {
        hotness.record(CallCount { target, count });
    }
    assert!(hotness.take_batch(|_| false).is_empty());
    hotness.record(CallCount {
        target: 1,
        count: 1,
    });
    assert_eq!(hotness.take_batch(|_| false), [1, 2]);
    assert!(hotness.take_batch(|_| false).is_empty());
}

#[test]
fn batches_keep_the_hottest_128_and_retain_excess_candidates() {
    let mut hotness = Hotness::default();
    for target in 0..129 {
        hotness.record(CallCount {
            target,
            count: 1024 + target,
        });
    }
    let batch = hotness.take_batch(|_| false);
    assert_eq!(batch, (1..=128).rev().collect::<Vec<_>>());
    assert!(hotness.take_batch(|_| false).is_empty());
    hotness.record(CallCount {
        target: 0,
        count: 3072,
    });
    assert_eq!(hotness.take_batch(|_| false), [0]);
}

#[test]
fn resolved_candidates_do_not_satisfy_the_minimum_or_force_a_batch() {
    let mut hotness = Hotness::default();
    for target in 0..64 {
        hotness.record(CallCount {
            target,
            count: 1024,
        });
    }
    hotness.record(CallCount {
        target: 0,
        count: 3072,
    });
    assert!(hotness.take_batch(|key| key == 0).is_empty());
    hotness.record(CallCount {
        target: 64,
        count: 1024,
    });
    assert_eq!(hotness.take_batch(|_| false), (1..=64).collect::<Vec<_>>());
}

async fn replace_fallback() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let memory = Memory::new(
        MachineConfig::new(
            0x10000,
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    memory.view().write(0x200, &[0x40, 0xc3]).unwrap(); // INC AX; RET
    memory.view().write(0x300, &[0x42, 0xe9, 0xfc, 0]).unwrap(); // INC DX; JMP 0400
    memory.view().write(0x7fe, &0x400u16.to_le_bytes()).unwrap();
    let mut state = CpuState {
        eip: 0x300,
        ..CpuState::real_mode()
    };
    state.registers.esp = 0x7fe;
    let mut cpu = Cpu::new(
        memory,
        CpuConfig::new(compiler.target(), state).unwrap(),
        Default::default(),
    )
    .await
    .unwrap();
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x400, async move || exit.request())])
        .await
        .unwrap();
    cpu.instance
        .interpret_at(crate::FarPointer {
            segment: 0,
            offset: 0x200,
        })
        .unwrap();
    cpu.instance
        .interpret_at(crate::FarPointer {
            segment: 0,
            offset: 0x300,
        })
        .unwrap();
    assert!(
        !cpu.has_entry(
            crate::FarPointer {
                segment: 1,
                offset: 0x1f0
            }
            .packed()
        ),
        "rejecting 0000:0200 must not suppress its 0001:01f0 alias"
    );
    let block = compiler.compile_block(0x200, &[0x40, 0xc3], 2).unwrap();
    cpu.install_module(
        &block.bytes,
        &[DispatchEntry {
            key: 0x200,
            export: &block.entry,
        }],
    )
    .await
    .unwrap();
    // Replacing one fallback must allocate a dynamic slot, leaving the shared
    // interpreter slot and the other rejected candidate untouched.
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!((cpu.cpu_state().ax(), cpu.cpu_state().dx()), (0, 1));
    cpu.cpu_state().set_eip(0x200);
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    assert_eq!((cpu.cpu_state().ax(), cpu.cpu_state().dx()), (1, 1));
    assert_eq!(cpu.cpu_state().sp(), 0x800);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replacing_a_rejected_candidate_preserves_other_interpreter_entries() {
    pollster::block_on(replace_fallback());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn replacing_a_rejected_candidate_preserves_other_interpreter_entries() {
    replace_fallback().await;
}
