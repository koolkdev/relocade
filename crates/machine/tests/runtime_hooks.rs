mod support;

use relocade_compiler::Program;
use relocade_machine::{real_mode_interrupt, DispatchEntry, Hook, HookResult};
use relocade_x86::{
    abi, Compiler, CpuState, ExecutionExit, ExecutionExitValues, ExecutionProfile, HostExit,
    PhysicalMapping, PhysicalMemoryMap, Segment, Segments,
};
use std::{cell::Cell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_interrupt_hooks() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x20000,
        PhysicalMemoryMap::new([(0..=0x1ffff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap(),
    )
    .unwrap();
    // Non-Send state belongs to the machine, while hooks may capture local metadata.
    let calls = Rc::new(Cell::new(0));
    let mut runtime = support::create_cpu(calls, config).await.unwrap();
    runtime.with(|call| call.cpu.set_sp(0xff00));
    runtime.with(|call| call.cpu.set_eflags(0x202));
    runtime.with(|call| {
        call.memory
            .write(
                0x100,
                &[
                    0xb8, 0xff, 0xff, // MOV AX,FFFF
                    0xc7, 0x06, 0, 2, 10, 0, // MOV word [0200],10
                    0x83, 0xc0, 2, // ADD AX,2 (sets carry)
                    0xcd, 0x21, // INT 21h
                    0xa3, 2, 2, // MOV [0202],AX
                    0xeb, 0xed, // JMP 0100
                ],
            )
            .unwrap()
    });
    runtime.with(|call| call.memory.write(0x84, &[0x20, 0, 0, 0x10]).unwrap());
    runtime.with(|call| {
        call.memory
            .write(0x10020, &real_mode_interrupt::HOOK_STUB)
            .unwrap()
    });
    let increment = Rc::new(Cell::new(1));
    runtime
        .install_hooks([
            Hook::<Rc<Cell<u32>>>::new(0x10020, async move |call| {
                call.state.set(call.state.get() + increment.get());
                assert_eq!(call.cpu.segment(Segment::Cs).selector, 0x1000);
                assert_eq!(call.cpu.eip(), 0x20);
                assert_eq!(call.cpu.sp(), 0xfefa);
                assert_eq!(call.cpu.ax(), 1);
                assert!(call.cpu.carry());
                assert_eq!(call.cpu.eflags() & 0x200, 0);
                let mut word = [0; 2];
                call.memory.read(0x200, &mut word).unwrap();
                call.cpu.set_ax(1 + u16::from_le_bytes(word));
                let mut frame = [0; 6];
                call.memory.read(0xfefa, &mut frame).unwrap();
                assert_eq!(&frame[..4], &[0x0e, 1, 0, 0]);
                assert_eq!(u16::from_le_bytes([frame[4], frame[5]]) & 0x201, 0x201);
                real_mode_interrupt::set_carry(call, false);
                real_mode_interrupt::return_from_interrupt(call)
            }),
            Hook::new(0x400, async |_| {
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }),
            Hook::new(0x401, async |_| {
                HookResult::Exit(ExecutionExit::Exception(
                    relocade_x86::Exception::PageFault {
                        linear_address: 0xfedc_ba98,
                        error_code: 0xdead_beef,
                    },
                ))
            }),
            Hook::new(0x402, async |_| {
                HookResult::Exit(ExecutionExit::UnsupportedInstruction {
                    eip: 0x89ab_cdef,
                    opcode: 0xf3,
                })
            }),
        ])
        .await
        .unwrap();
    assert_eq!(runtime.run(5).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.with(|call| call.state.get()), 1);
    assert_eq!(runtime.with(|call| call.cpu.ax()), 11);
    assert_eq!(
        runtime.with(|call| call.cpu.segment(Segment::Cs).selector),
        0
    );
    assert_eq!(runtime.with(|call| call.cpu.eip()), 0x10e);
    assert_eq!(runtime.with(|call| call.cpu.sp()), 0xff00);
    assert_eq!(runtime.with(|call| call.cpu.eflags()) & 0x201, 0x200);
    assert_eq!(runtime.run(2).await, ExecutionExit::BudgetExhausted);
    let mut result = [0; 2];
    runtime.with(|call| call.memory.read(0x202, &mut result).unwrap());
    assert_eq!(u16::from_le_bytes(result), 11);

    // Replace the IVT destination with guest code that chains to the original hook.
    runtime.with(|call| call.memory.write(0x84, &[0, 5, 0, 0]).unwrap());
    runtime.with(|call| {
        call.memory
            .write(0x500, &[0x43, 0xea, 0x20, 0, 0, 0x10])
            .unwrap()
    });
    assert_eq!(runtime.run(7).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.with(|call| call.state.get()), 2);
    assert_eq!(runtime.with(|call| call.cpu.bx()), 1);
    assert_eq!(runtime.with(|call| call.cpu.eip()), 0x10e);
    assert_eq!(runtime.with(|call| call.cpu.sp()), 0xff00);
    assert_eq!(runtime.with(|call| call.cpu.instruction_count()), 14);

    for (address, expected) in [
        (0x400, ExecutionExit::Host(HostExit::Return)),
        (
            0x401,
            ExecutionExit::Exception(relocade_x86::Exception::PageFault {
                linear_address: 0xfedc_ba98,
                error_code: 0xdead_beef,
            }),
        ),
        (
            0x402,
            ExecutionExit::UnsupportedInstruction {
                eip: 0x89ab_cdef,
                opcode: 0xf3,
            },
        ),
    ] {
        runtime.with(|call| call.cpu.set_eip(address));
        assert_eq!(runtime.run(1).await, expected);
        assert_eq!(runtime.with(|call| call.cpu.instruction_count()), 14);
        assert_eq!(runtime.with(|call| call.cpu.eip()), address);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn interrupt_hooks_return_through_guest_frames_and_allow_chaining() {
    pollster::block_on(check_interrupt_hooks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn interrupt_hooks_return_through_guest_frames_and_allow_chaining() {
    check_interrupt_hooks().await;
}

async fn check_mapped_hook_access() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x3000,
        PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Rom { backing_offset: 0 }),
            (
                0x1000..=0x1fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x2000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu(0_u32, config).await.unwrap();
    cpu.with(|call| call.cpu.set_eax(40));
    cpu.with(|call| call.memory.write(0x1020, &10_u32.to_le_bytes()).unwrap());
    cpu.with(|call| call.memory.write_backing(0x20, &[0xa5]).unwrap());
    cpu.with(|call| {
        call.memory
            .write_backing(
                0x100,
                &[
                    0x66, 0x40, // INC EAX
                    0x66, 0xff, 0x06, 0x20, 0x10, // INC dword [1020]
                    0xe9, 0xf6, 0, // JMP 0200 (hook)
                ],
            )
            .unwrap()
    });
    cpu.with(|call| {
        call.memory
            .write_backing(
                0x300,
                &[
                    0x66, 0x8b, 0x1e, 0x24, 0x10, // MOV EBX,[1024]
                    0xe9, 0xf8, 0xfd, // JMP 0100
                ],
            )
            .unwrap()
    });
    cpu.install_hooks([Hook::new(0x200, async |call| {
        let mut bytes = [0; 4];
        call.memory.read(0x1020, &mut bytes).unwrap();
        let value = call.cpu.eax() + u32::from_le_bytes(bytes);
        call.cpu.set_eax(value);
        call.cpu.set_eip(0x300);
        call.memory.write(0x1024, &value.to_le_bytes()).unwrap();
        call.memory.write_u8(0x20, 0xff).unwrap();
        *call.state += 1;
        HookResult::Continue
    })])
    .await
    .unwrap();

    assert_eq!(cpu.with(|call| *call.state), 0);
    assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(cpu.with(|call| *call.state), 0);
    for (calls, value) in [(1, 52), (2, 65)] {
        assert_eq!(cpu.run(5).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.with(|call| *call.state), calls);
        assert_eq!(cpu.with(|call| call.cpu.eax()), value);
        assert_eq!(cpu.with(|call| call.cpu.ebx()), value);
        assert_eq!(cpu.with(|call| call.cpu.eip()), 0x100);
        let mut bytes = [0; 4];
        cpu.with(|call| call.memory.read(0x1024, &mut bytes).unwrap());
        assert_eq!(u32::from_le_bytes(bytes), value);
        assert_eq!(cpu.with(|call| call.memory.read_u8(0x20).unwrap()), 0xa5);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn hooks_edit_published_cpu_and_mapped_memory_during_runs() {
    pollster::block_on(check_mapped_hook_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn hooks_edit_published_cpu_and_mapped_memory_during_runs() {
    check_mapped_hook_access().await;
}

async fn check_hook_replacement() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x400,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = support::create_cpu((), config).await.unwrap();
    runtime.with(|call| call.memory.write(0x100, &[0x43, 0xe9, 0xfc, 2]).unwrap()); // INC BX; JMP 0400
    let first = Rc::new(Cell::new(0));
    let second = Rc::new(Cell::new(0));
    let released = Rc::downgrade(&first);
    let calls = Rc::clone(&second);
    runtime
        .install_hooks([
            Hook::new(0x400, async move |call| {
                first.set(first.get() + 1);
                call.cpu.set_ax(first.get());
                call.cpu.set_eip(0x401);
                HookResult::Continue
            }),
            Hook::new(0x401, async move |call| {
                calls.set(calls.get() + 1);
                call.cpu.set_eip(0x100);
                HookResult::Continue
            }),
        ])
        .await
        .unwrap();
    // Both hooks consume no instructions. Reentering Wasm retains one target.
    assert_eq!(runtime.run(3).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.with(|call| call.cpu.instruction_count()), 4);
    assert_eq!(runtime.with(|call| call.cpu.ax()), 2);
    assert_eq!(runtime.with(|call| call.cpu.bx()), 2);
    assert_eq!(second.get(), 2);
    runtime
        .install_hooks([Hook::new(0x400, async |call| {
            call.cpu.set_ax(50);
            call.cpu.set_eip(0x401);
            HookResult::Continue
        })])
        .await
        .unwrap();
    assert!(released.upgrade().is_none());
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.with(|call| call.cpu.ax()), 50);
    assert_eq!(second.get(), 3);

    let mut program = Program::new();
    let entry = program.function::<abi::ExecutionSignature>(|body, ()| {
        body.return_(ExecutionExitValues::host(HostExit::Return))
    });
    program.export("entry", entry).unwrap();
    let bytes = program.compile().unwrap();
    // An unresolved later export preserves both the hook and its captured state.
    assert!(runtime
        .install_module(
            &bytes,
            &[
                DispatchEntry {
                    address: 0x401,
                    export: "entry"
                },
                DispatchEntry {
                    address: 0x402,
                    export: "missing"
                },
            ]
        )
        .await
        .is_err());
    runtime.with(|call| call.cpu.set_eip(0x401));
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(second.get(), 4);
    assert_eq!(Rc::strong_count(&second), 2);
    runtime
        .install_module(
            &bytes,
            &[DispatchEntry {
                address: 0x401,
                export: "entry",
            }],
        )
        .await
        .unwrap();
    assert_eq!(Rc::strong_count(&second), 1);
    runtime.with(|call| call.cpu.set_eip(0x401));
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(second.get(), 4);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn local_hooks_preserve_budgets_and_release_captures_when_replaced() {
    pollster::block_on(check_hook_replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn local_hooks_preserve_budgets_and_release_captures_when_replaced() {
    check_hook_replacement().await;
}
