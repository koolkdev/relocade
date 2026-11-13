mod support;

use relocade_machine::{Cpu, DispatchEntry, FarPointer, Hook};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, FunctionLimits, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segment,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn cpu() -> Cpu {
    support::create_cpu(
        support::configuration(
            Compiler::new(ExecutionProfile::Real16)
                .with_instruction_budgets(true)
                .target(),
            CpuState {
                eip: 0x100,
                ..CpuState::real_mode()
            },
            0x10000,
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .await
    .unwrap()
}

fn target(offset: u16) -> FarPointer {
    FarPointer { segment: 0, offset }
}

async fn calls() {
    for (compiled_caller, compiled_callee) in [(false, false), (true, false), (true, true)] {
        let mut cpu = cpu().await;
        cpu.cpu_state().set_sp(0x800);
        let caller = [
            0xb9, 3, 0, // MOV CX,3
            0xe8, 0xfa, 0, // CALL 0200
            0xbb, 0x10, 2, // MOV BX,0210
            0xff, 0xd3, // CALL BX
            0xe2, 0xf6, // LOOP 0103
            0xe8, 0xf0, 1, // CALL 0300 (host hook)
        ];
        let memory = cpu.memory();
        memory.view().write(0x100, &caller).unwrap();
        memory.view().write(0x200, &[0x40, 0xc3]).unwrap(); // INC AX; RET
        memory.view().write(0x210, &[0x42, 0xc3]).unwrap(); // INC DX; RET
        let exit = cpu.exit_handle();
        cpu.install([Hook::host(0x300, async move || exit.request())])
            .await
            .unwrap();
        let compiler = Compiler::new(ExecutionProfile::Real16).with_instruction_budgets(true);
        if compiled_caller {
            let block = compiler.compile_block(0x100, &caller, 2).unwrap();
            cpu.install_module(
                &block.bytes,
                &[DispatchEntry {
                    address: 0x100,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
        }
        if compiled_callee {
            let block = compiler.compile_block(0x200, &[0x40, 0xc3], 2).unwrap();
            cpu.install_module(
                &block.bytes,
                &[DispatchEntry {
                    address: 0x200,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
        }
        // Stop just after the first call; requesting its count collects the small batch.
        cpu.instruction_target().set(Some(2));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.cpu_state().eip(), 0x200);
        assert_eq!(cpu.call_count(target(0x200)), u64::from(!compiled_callee));
        cpu.instruction_target().set(None);
        assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(
            cpu.call_count(target(0x200)),
            if compiled_callee { 0 } else { 3 }
        );
        assert_eq!(cpu.call_count(target(0x210)), 3);
        assert_eq!(cpu.call_count(target(0x300)), 0);
        assert_eq!(
            (
                cpu.cpu_state().ax(),
                cpu.cpu_state().dx(),
                cpu.cpu_state().sp()
            ),
            (3, 3, 0x7fe)
        );
    }
}

async fn far_calls() {
    let first = FarPointer {
        segment: 0x10,
        offset: 0x200,
    };
    let second = FarPointer {
        segment: 0x20,
        offset: 0x210,
    };
    let installed = FarPointer {
        segment: 0x20,
        offset: 0x300,
    };
    let hooked = FarPointer {
        segment: 0x30,
        offset: 0x400,
    };
    for compiled_caller in [false, true] {
        let mut cpu = cpu().await;
        cpu.cpu_state().set_sp(0x800);
        let caller = [
            0x9a, 0, 2, 0x10, 0, // CALL FAR 0010:0200
            0x66, 0xff, 0x1e, 0, 4, // CALL FAR dword [0400]
            0x9a, 0, 3, 0x20, 0, // CALL FAR 0020:0300 (installed)
            0x9a, 0, 4, 0x30, 0, // CALL FAR 0030:0400 (host hook)
            0xc3,
        ];
        let memory = cpu.memory();
        memory.view().write(0x100, &caller).unwrap();
        memory.view().write(0x300, &[0x40, 0xcb]).unwrap(); // INC AX; RETF
        memory
            .view()
            .write(0x400, &[0x10, 2, 0, 0, 0x20, 0])
            .unwrap();
        memory.view().write(0x410, &[0x42, 0x66, 0xcb]).unwrap(); // INC DX; RETFD
        memory.view().write(0x500, &[0x43, 0xcb]).unwrap(); // INC BX; RETF
        let exit = cpu.exit_handle();
        cpu.install([Hook::host(0x700, async move || exit.request())])
            .await
            .unwrap();
        let compiler = Compiler::new(ExecutionProfile::Real16).with_instruction_budgets(true);
        let callee = compiler.compile_block(0x300, &[0x43, 0xcb], 2).unwrap();
        cpu.install_module(
            &callee.bytes,
            &[DispatchEntry {
                address: 0x500,
                export: &callee.entry,
            }],
        )
        .await
        .unwrap();
        if compiled_caller {
            let function = compiler
                .compile_function(0x100, FunctionLimits::default(), |eip, output| {
                    let Some(bytes) = caller.get(eip.wrapping_sub(0x100) as usize..) else {
                        return 0;
                    };
                    let len = bytes.len().min(output.len());
                    output[..len].copy_from_slice(&bytes[..len]);
                    len
                })
                .unwrap();
            let entries: Vec<_> = function
                .entries
                .iter()
                .map(|block| DispatchEntry {
                    address: block.start_eip,
                    export: &block.export,
                })
                .collect();
            cpu.install_module(&function.bytes, &entries).await.unwrap();
        }
        // The completed call is visible even when its checkpoint returns first.
        cpu.instruction_target().set(Some(1));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.cpu_state().segment(Segment::Cs).selector, first.segment);
        assert_eq!(cpu.cpu_state().eip(), u32::from(first.offset));
        assert_eq!(cpu.cpu_state().sp(), 0x7fc);
        assert_eq!(cpu.call_count(first), 1);
        assert_eq!(cpu.call_count(target(first.offset)), 0);
        cpu.instruction_target().set(None);
        assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(cpu.call_count(first), 1);
        assert_eq!(cpu.call_count(second), 1);
        assert_eq!(cpu.call_count(installed), 0);
        assert_eq!(cpu.call_count(hooked), 0);
        assert_eq!(
            (
                cpu.cpu_state().ax(),
                cpu.cpu_state().dx(),
                cpu.cpu_state().bx()
            ),
            (1, 1, 1)
        );
        assert_eq!(cpu.cpu_state().sp(), 0x7fc);
    }
}

async fn overflow() {
    let mut cpu = cpu().await;
    cpu.cpu_state().set_sp(0x800);
    let targets: Vec<u16> = (0x4000u32..0xe000)
        .filter(|target| {
            let product = target.wrapping_mul(0x9e37_79b1);
            (product ^ (product >> 16)) & 1023 == 0
        })
        .take(9)
        .map(|target| target as u16)
        .collect();
    assert_eq!(targets.len(), 9);
    let mut code = Vec::new();
    let memory = cpu.memory();
    for &callee in &targets {
        let displacement = callee.wrapping_sub(0x100 + code.len() as u16 + 3);
        code.push(0xe8);
        code.extend(displacement.to_le_bytes());
        memory.view().write(u32::from(callee), &[0xc3]).unwrap();
    }
    code.push(0xe9);
    code.extend((0xf000u16 - (0x100 + code.len() as u16 + 2)).to_le_bytes());
    memory.view().write(0x100, &code).unwrap();
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0xf000, async move || exit.request())])
        .await
        .unwrap();
    assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
    for callee in targets {
        assert_eq!(cpu.call_count(target(callee)), 1);
    }
    assert_eq!(cpu.cpu_state().sp(), 0x800);
    assert_eq!(cpu.cpu_state().instruction_count(), 19);
}

async fn failed_and_stepped_calls() {
    for (code, destination) in [
        (&[0xe8, 0xfd, 0][..], target(0x200)),
        (
            &[0x9a, 0, 2, 0x10, 0],
            FarPointer {
                segment: 0x10,
                offset: 0x200,
            },
        ),
    ] {
        let mut cpu = cpu().await;
        cpu.memory().view().write(0x100, code).unwrap();
        // A wrapping push crosses the real-mode SS limit and does not complete CALL.
        cpu.cpu_state().set_sp(1);
        assert!(matches!(cpu.step(), ExecutionExit::Exception(_)));
        assert_eq!(cpu.call_count(destination), 0);
        assert_eq!(cpu.cpu_state().eip(), 0x100);
        assert_eq!(cpu.cpu_state().segment(Segment::Cs).selector, 0);
        cpu.cpu_state().set_sp(0x800);
        cpu.interrupt_handle().request();
        assert_eq!(cpu.step(), ExecutionExit::Interrupted);
        assert_eq!(cpu.call_count(destination), 1);
        assert_eq!(cpu.cpu_state().eip(), u32::from(destination.offset));
        assert_eq!(
            cpu.cpu_state().segment(Segment::Cs).selector,
            destination.segment
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completed_call_misses_merge_across_execution_boundaries() {
    pollster::block_on(async {
        calls().await;
        far_calls().await;
        overflow().await;
        failed_and_stepped_calls().await;
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn completed_call_misses_merge_across_execution_boundaries() {
    calls().await;
    far_calls().await;
    overflow().await;
    failed_and_stepped_calls().await;
}
