use super::*;
use wasm86_runtime::{SliceOutcome, SliceStop};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn interrupt_calls_host_with_real_frame_and_returns_through_iret() {
    let mut machine = machine(&[0xcd, 0x21]).await;
    let mut cpu = machine.cpu();
    cpu.flags.bytes.if_ = 1;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0x84, &[0, 0, 0, 1]).unwrap();
    // The target contains no guest IRET; the hook module owns its return.
    install(&mut machine, 0x1000, HookReturn::Interrupt, |host| {
        let mut cpu = host.cpu();
        assert_eq!(
            (cpu.segments.cs.selector, cpu.eip, cpu.registers.esp),
            (0x100, 0, 0xfefa)
        );
        assert_eq!(cpu.instruction_count, 1);
        assert_eq!(cpu.flags.bytes.if_, 0);
        let mut frame = [0; 6];
        host.read_memory(0xfefa, &mut frame).unwrap();
        assert_eq!(frame, [2, 1, 0, 0, 2, 2]);
        host.write_memory(0xfefe, &[3, 2]).unwrap(); // return CF in saved FLAGS
        cpu.registers.eax = 0xbeef;
        host.set_cpu(cpu).unwrap();
        HostAction::Continue
    })
    .await;
    assert_eq!(
        machine.run_slice(3).unwrap(),
        SliceOutcome {
            completed: 2,
            work: 3,
            stop: SliceStop::WorkLimit,
        }
    );
    let cpu = machine.cpu();
    assert_eq!(
        (cpu.segments.cs.selector, cpu.eip, cpu.registers.esp),
        (0, 0x102, 0xff00)
    );
    assert_eq!(cpu.registers.eax, 0xbeef);
    assert_eq!((cpu.flags.bytes.if_, cpu.flags.bytes.cf & 1), (1, 1));
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rewritten_vector_runs_guest_handler_and_can_chain_to_host_target() {
    let mut machine = machine(&[0xcd, 0x21]).await;
    machine.write_memory(0x84, &[0, 0, 0, 2]).unwrap();
    // Guest handler: INC AX; JMP FAR 0100:0000, retaining the INT frame.
    machine
        .write_memory(0x2000, &[0x40, 0xea, 0, 0, 0, 1])
        .unwrap();
    // The target contains no guest IRET; the hook module owns its return.
    let calls = count_calls(&mut machine, 0x1000, HostAction::Continue).await;
    assert_eq!(machine.run(2, |_| false).unwrap().completed, 2);
    assert_eq!(calls.get(), 0);
    assert_eq!(machine.cpu().registers.eax, 1);
    assert_eq!(machine.run(2, |_| false).unwrap().completed, 2);
    assert_eq!(calls.get(), 1);
    assert_eq!(
        (machine.cpu().eip, machine.cpu().registers.esp),
        (0x102, 0xff00)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn ordinary_call_uses_compiled_near_return_and_stack_cleanup() {
    let mut machine = machine(&[0xe8, 0xfd, 0]).await; // CALL 0200
    install(
        &mut machine,
        0x200,
        HookReturn::Near { pop_bytes: 4 },
        |host| {
            let mut return_address = [0; 2];
            host.read_memory(host.cpu().registers.esp, &mut return_address)
                .unwrap();
            assert_eq!(return_address, [3, 1]);
            let mut cpu = host.cpu();
            cpu.registers.eax = 42;
            host.set_cpu(cpu).unwrap();
            HostAction::Continue
        },
    )
    .await;
    assert_eq!(machine.run(2, |_| false).unwrap().completed, 2);
    assert_eq!(
        (
            machine.cpu().eip,
            machine.cpu().registers.esp,
            machine.cpu().registers.eax
        ),
        (0x103, 0xff04, 42)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn far_return_observes_routing_changes_published_by_the_host_call() {
    let mut machine = machine(&[0x9a, 0, 0, 0, 1]).await; // CALL FAR 0100:0000
    install(
        &mut machine,
        0x1000,
        HookReturn::Far { pop_bytes: 2 },
        |host| {
            host.with_memory(|memory| {
                // The callback redirects the stack page to backing containing a
                // different far return address. The generated RETF must use it.
                let map = wasm86_x86::PhysicalMemoryMap::new([
                    (
                        0..=0x10ffff,
                        wasm86_x86::PhysicalMapping::Ram { backing_offset: 0 },
                    ),
                    (
                        0xf000..=0xffff,
                        wasm86_x86::PhysicalMapping::Ram {
                            backing_offset: 0x20000,
                        },
                    ),
                ])
                .unwrap();
                memory.write_backing(0x20efc, &[0x30, 0, 0, 2]).unwrap();
                memory.set_map(map).unwrap();
            });
            let mut cpu = host.cpu();
            cpu.instruction_count = 100; // Host state edits cannot forge retirement.
            host.set_cpu(cpu).unwrap();
            HostAction::Continue
        },
    )
    .await;
    assert_eq!(machine.run_slice(3).unwrap().completed, 2);
    let cpu = machine.cpu();
    assert_eq!(
        (cpu.segments.cs.selector, cpu.eip, cpu.registers.esp),
        (0x200, 0x30, 0xff02)
    );
    assert_eq!(cpu.instruction_count, 2);
}
