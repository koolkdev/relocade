use super::collected_count;
use crate::{
    CompilerService, CpuConfig, ExecutionWorker, FarPointer, Hook, Instant, Machine, MachineConfig,
    Runner, SessionConfig,
};
use futures_util::future::Either;
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{num::NonZeroUsize, time::Duration};

mod suspended;

fn setup() -> (Machine, CpuConfig) {
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
        (0x1000..=0x1fff, PhysicalMapping::MMIO),
    ])
    .unwrap();
    let machine = Machine::new(MachineConfig::new(0x2000, map).unwrap()).unwrap();
    (machine, config)
}

async fn slice(runner: &mut Runner) {
    let elapsed = runner.clock().sleep(Duration::from_millis(20));
    assert!(matches!(
        runner.run_cpu_until(elapsed).await,
        Either::Right(())
    ));
}

async fn automatic() {
    let compiler = CompilerService::start_with_workers(NonZeroUsize::new(1).unwrap()).unwrap();
    full_collection(&compiler).await;
    // One root in the second batch branches to an unsupported instruction.
    // Its valid fallthrough still runs in the compiled module.
    for partial in [false, true] {
        let (mut machine, config) = setup();
        machine
            .memory()
            .view()
            .write(
                0x100,
                if partial {
                    &[0xe8, 0xfd, 0, 0xe8, 0xfa, 1, 0xeb, 0xf8] // CALL 0200; CALL 0300; JMP 0100
                } else {
                    &[0xe8, 0xfd, 0, 0xeb, 0xfb] // CALL 0200; JMP 0100
                },
            )
            .unwrap();
        let callee: &[u8] = if partial {
            &[0x40, 0x85, 0xff, 0x75, 0x1b, 0xc3] // INC AX; TEST DI,DI; JNZ 0220; RET
        } else {
            &[0x40, 0xc3] // INC AX; RET
        };
        machine.memory().view().write(0x200, callee).unwrap();
        machine.memory().view().write(0x220, &[0xf1]).unwrap();
        machine.memory().view().write(0x300, &[0x42, 0xc3]).unwrap(); // INC DX; RET
        let cpu = machine.create_cpu(config, Some(&compiler)).unwrap();
        let result = ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
            runner.cpu().cpu_state().set_sp(0x800);
            let targets: Vec<_> = [0x200, 0x300]
                .into_iter()
                .take(if partial { 2 } else { 1 })
                .map(|offset| FarPointer { segment: 0, offset })
                .collect();
            let deadline = runner
                .clock()
                .deadline_after(runner.clock().now(), Duration::from_secs(5));
            loop {
                let retired = runner.cpu().cpu_state().instruction_count();
                slice(&mut runner).await;
                assert!(runner.cpu().cpu_state().instruction_count() > retired);
                if targets
                    .iter()
                    .all(|entry| runner.cpu().instance.modules.has_entry(entry.packed()))
                {
                    for entry in &targets {
                        assert_eq!(
                            runner
                                .cpu()
                                .compiled_modules_at(u32::from(entry.offset))
                                .count(),
                            1,
                        );
                    }
                    if partial {
                        // The RET after JNZ is an internal fallthrough.
                        assert!(!runner.cpu().instance.modules.has_entry(0x205));
                        assert_eq!(runner.cpu().compiled_modules_at(0x205).count(), 1);
                    }
                    assert_eq!(runner.cpu().compiled_modules_at(0x220).count(), 0);
                    // Wait for installation before inspecting counts, so this test
                    // cannot drive hotness by collecting the buffer itself.
                    let counts: Vec<_> = targets
                        .iter()
                        .map(|&entry| collected_count(runner.cpu(), entry))
                        .collect();
                    assert!(counts.iter().any(|&count| count >= 4096));
                    let retired = runner.cpu().cpu_state().instruction_count();
                    slice(&mut runner).await;
                    assert!(runner.cpu().cpu_state().instruction_count() > retired);
                    assert_eq!(
                        targets
                            .iter()
                            .map(|&entry| collected_count(runner.cpu(), entry))
                            .collect::<Vec<_>>(),
                        counts
                    );
                    break;
                }
                assert!(
                    runner.clock().now() < deadline,
                    "automatic installation did not complete"
                );
            }
        })
        .join()
        .await;
        machine.shutdown().await.unwrap();
        assert_eq!(result.unwrap(), Some(()));
    }
    compiler.shutdown().await.unwrap();
}

async fn full_collection(compiler: &CompilerService) {
    let (mut machine, config) = setup();
    // Repeated calls make one callee hot without filling the table. Distinct
    // callees then force a full-buffer collection before the final host exit.
    let mut caller = vec![
        0xb9, 0, 0x20, // MOV CX,8192
        0xe8, 0xfa, 7, // CALL 0900
        0xe2, 0xfb, // LOOP 0103
    ];
    for target in 0x900u16..0xb58 {
        let displacement = target.wrapping_sub(0x100 + caller.len() as u16 + 3);
        caller.push(0xe8);
        caller.extend(displacement.to_le_bytes());
        machine
            .memory()
            .view()
            .write(u32::from(target), &[0xc3])
            .unwrap();
    }
    let displacement = 0xf00u16 - (0x100 + caller.len() as u16 + 3);
    caller.push(0xe9);
    caller.extend(displacement.to_le_bytes());
    machine.memory().view().write(0x100, &caller).unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let result = ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let target = FarPointer {
            segment: 0,
            offset: 0x900,
        };
        runner.cpu().cpu_state().set_sp(0xe00);
        // No opportunistic collection can submit during this run.
        runner.cpu().instance.profiling.next_collection =
            Some(Instant::now() + Duration::from_secs(3600));
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0xf00, async move || exit.request())])
            .await
            .unwrap();
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        let cpu = runner.cpu();
        assert!(cpu.instance.profiling.hotness.count(target.packed()) >= 4096);
        assert!(
            cpu.instance.profiling.pending.contains(&target)
                || cpu.instance.modules.has_entry(target.packed()),
            "full-buffer collection must submit before guest execution returns"
        );
    })
    .join()
    .await;
    machine.shutdown().await.unwrap();
    assert_eq!(result.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn collection_compiles_hot_functions_with_partially_discovered_branches() {
    pollster::block_on(automatic());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn collection_compiles_hot_functions_with_partially_discovered_branches() {
    automatic().await;
}
