#![cfg(feature = "worker")]

use futures_lite::future::poll_once;
use relocade_machine::{
    BlockRequest, CompilerWorker, Cpu, CpuConfig, DispatchEntry, Error, MachineConfig, Memory,
    MemoryError,
};
use relocade_x86::{
    BlockError, Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};
use std::pin::pin;

fn block(address: u32, start_eip: u32, byte_len: usize, instruction_limit: u32) -> BlockRequest {
    BlockRequest {
        address,
        start_eip,
        byte_len,
        instruction_limit,
    }
}

async fn check_compiler() {
    let memory = Memory::new(
        MachineConfig::new(
            0x4000,
            PhysicalMemoryMap::new([
                (
                    0x1000..=0x1fff,
                    PhysicalMapping::Ram {
                        backing_offset: 0x2000,
                    },
                ),
                (0x2000..=0x2fff, PhysicalMapping::Rom { backing_offset: 0 }),
                (0x4000..=0x4fff, PhysicalMapping::Mmio),
            ])
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut worker = CompilerWorker::start(&memory, target).await.unwrap();
    // Writes after startup must be visible through the worker's shared backing.
    memory
        .view()
        .write(0x1100, &[0x40, 0xeb, 0x0d, 0x0f])
        .unwrap();
    memory.view().write(0x1110, &[0x43, 0xeb, 0xed]).unwrap();
    assert!(matches!(
        worker
            .compile_blocks(vec![block(0x1100, 0x100, 4, 0)])
            .await,
        Err(Error::Compile(BlockError::ZeroInstructionLimit))
    ));
    for address in [0x3000, 0x4000] {
        assert!(matches!(
            worker.compile_blocks(vec![block(address, 0, 1, 1)]).await,
            Err(Error::Memory(MemoryError::UnbackedRange))
        ));
    }
    assert!(matches!(
        worker
            .compile_blocks(vec![block(2, 0, usize::MAX, 1)])
            .await,
        Err(Error::Memory(MemoryError::AddressRange))
    ));
    // Canceling one submission leaves the persistent worker usable.
    {
        let mut abandoned = pin!(worker.compile_blocks(vec![block(0x1100, 0x100, 1, 1)]));
        let _ = poll_once(&mut abandoned).await;
    }
    let compiled = worker
        .compile_blocks(vec![
            block(0x1100, 0x100, 4, 1),
            block(0x1110, 0x110, 3, 10),
        ])
        .await
        .unwrap();
    assert_eq!(compiled.entries.len(), 2);
    assert_eq!(compiled.entries[0].byte_len, 1);
    assert_eq!(compiled.entries[1].byte_len, 3);
    assert_eq!(compiled.execution_target, target);

    // Capture traverses noncontiguous RAM/ROM backing, with no device bus.
    memory.view().write(0x1fff, &[0xb8]).unwrap();
    memory.view().write_backing(0, &[0x34, 0x12]).unwrap();
    let crossing = worker
        .compile_blocks(vec![block(0x1fff, 0xfff, 3, 1)])
        .await
        .unwrap();
    assert_eq!(crossing.entries[0].byte_len, 3);
    worker.shutdown().await.unwrap();

    let mut segments = Segments::real_mode();
    segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x100);
    let config = CpuConfig::new(
        target,
        CpuState {
            eip: 0x100,
            segments,
            ..Default::default()
        },
    )
    .unwrap();
    let mut cpu = Cpu::new(memory, config, Default::default()).await.unwrap();
    let entries: Vec<_> = compiled
        .entries
        .iter()
        .map(|entry| DispatchEntry {
            address: 0x1000 + entry.start_eip,
            export: &entry.export,
        })
        .collect();
    cpu.install_compiled_module(&compiled.module, &entries)
        .await
        .unwrap();
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        cpu.cpu_state().eip(),
        0x101,
        "compiled prefix ends before JMP"
    );
    assert_eq!(cpu.cpu_state().ax(), 1);
    cpu.cpu_state().set_eip(0x110);
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        cpu.cpu_state().eip(),
        0x100,
        "compiled block also executes JMP"
    );
    assert_eq!(cpu.cpu_state().bx(), 1);
    cpu.install_compiled_module(
        &crossing.module,
        &[DispatchEntry {
            address: 0x1fff,
            export: &crossing.entries[0].export,
        }],
    )
    .await
    .unwrap();
    cpu.cpu_state().set_eip(0xfff);
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().ax(), 0x1234);
    assert_eq!(cpu.cpu_state().eip(), 0x1002);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_compiler_reads_mapped_guest_code_and_outlives_requests() {
    pollster::block_on(check_compiler());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn persistent_compiler_reads_mapped_guest_code_and_outlives_requests() {
    check_compiler().await;
}
