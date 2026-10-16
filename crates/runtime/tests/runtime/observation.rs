use super::machine;
use wasm86_runtime::{SliceOutcome, SliceStop};
use wasm86_x86::{Exception, ExecutionExit, Segment, StoredSegment};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn work_allowance_stops_a_loop_and_resumes_across_counter_wrap() {
    let mut machine = machine(&[0x40, 0xeb, 0xfd]).await; // INC AX; JMP 0100
    let mut cpu = machine.cpu();
    cpu.instruction_count = u64::MAX - 1;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine.run_slice_until(6, |_| false).unwrap(),
        SliceOutcome {
            work: 6,
            completed: 6,
            stop: SliceStop::WorkLimit
        }
    );
    assert_eq!(machine.cpu().registers.eax, 3);
    assert_eq!(machine.cpu().instruction_count, 4);
    assert_eq!(machine.cpu().eip, 0x100);
    assert_eq!(machine.run_slice_until(2, |_| false).unwrap().completed, 2);
    assert_eq!(machine.cpu().registers.eax, 4);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn host_can_stop_at_an_interrupt_handler_and_resume_its_return() {
    let mut machine = machine(&[0xcd, 0x21, 0x0f, 0x0b]).await;
    machine.write_memory(0x84, &[0x10, 0, 0x34, 0x12]).unwrap();
    machine.write_memory(0x12350, &[0xcf]).unwrap();
    let at_handler = |m: &super::TestMachine| {
        let cpu = m.cpu();
        cpu.segments.cs.selector == 0x1234 && cpu.eip == 0x10
    };
    assert_eq!(
        machine.run_slice_until(1, at_handler).unwrap(),
        SliceOutcome {
            work: 1,
            completed: 1,
            stop: SliceStop::HostRequested
        }
    );
    // Repeated polling does not enter the handler or replay INT.
    assert_eq!(
        machine.run_slice_until(100, at_handler).unwrap().completed,
        0
    );
    let mut cpu = machine.cpu();
    cpu.registers.eax = 0x1234;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine
            .run_slice_until(1, |m| m.cpu().segments.cs.selector == 0)
            .unwrap(),
        SliceOutcome {
            work: 1,
            completed: 1,
            stop: SliceStop::HostRequested
        }
    );
    assert_eq!(machine.cpu().eip, 0x102);
    assert_eq!(machine.cpu().registers.esp, 0xff00);
    assert_eq!(machine.cpu().registers.eax, 0x1234);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn guest_exit_preserves_prior_work_and_takes_precedence_over_host_polling() {
    for (suffix, exit) in [
        (
            &[0x0f, 0x0b][..],
            ExecutionExit::Exception(Exception::InvalidOpcode),
        ),
        (
            &[0xf4][..],
            ExecutionExit::UnsupportedInstruction {
                eip: 0x101,
                opcode: 0xf4,
            },
        ),
    ] {
        let mut code = vec![0x40];
        code.extend_from_slice(suffix);
        let mut machine = machine(&code).await;
        let mut polls = 0;
        let outcome = machine
            .run_slice_until(10, |_| {
                polls += 1;
                polls > 2
            })
            .unwrap();
        assert_eq!(
            outcome,
            SliceOutcome {
                work: 1,
                completed: 1,
                stop: SliceStop::Exited(exit)
            }
        );
        assert_eq!(polls, 2);
        assert_eq!(machine.cpu().registers.eax, 1);
        assert_eq!(machine.cpu().eip, 0x101);
        assert_eq!(machine.cpu().instruction_count, 1);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rep_is_one_completed_instruction() {
    let mut machine = machine(&[0xf3, 0xa4, 0xf4]).await; // REP MOVSB; unsupported HLT
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0x300;
    cpu.registers.edi = 0x400;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0x300, &[0x12, 0x34, 0x56]).unwrap();
    assert_eq!(
        machine.run_slice_until(1, |_| false).unwrap(),
        SliceOutcome {
            work: 3,
            completed: 1,
            stop: SliceStop::WorkLimit
        }
    );
    let mut bytes = [0; 3];
    machine.read_memory(0x400, &mut bytes).unwrap();
    assert_eq!(bytes, [0x12, 0x34, 0x56]);
    assert_eq!(machine.cpu().registers.ecx, 0);
    assert_eq!(machine.cpu().eip, 0x102);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn a_partial_rep_fault_is_not_counted_and_can_resume_after_repair() {
    let mut machine = machine(&[0x67, 0xf3, 0xa5]).await; // REP MOVSW with ESI/EDI/ECX
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0xfffe;
    cpu.registers.edi = 0x200;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0xfffe, &[0x12, 0x34]).unwrap();
    assert_eq!(
        machine.run_slice_until(10, |_| false).unwrap(),
        SliceOutcome {
            work: 1,
            completed: 0,
            stop: SliceStop::Exited(ExecutionExit::Exception(Exception::GeneralProtection {
                error_code: 0
            })),
        }
    );
    let mut bytes = [0; 4];
    machine.read_memory(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0x12, 0x34, 0, 0]);
    let mut cpu = machine.cpu();
    assert_eq!(
        (
            cpu.eip,
            cpu.registers.ecx,
            cpu.registers.esi,
            cpu.registers.edi
        ),
        (0x100, 1, 0x10000, 0x202)
    );
    // Repair the remaining source pointer without replaying the completed element.
    cpu.registers.esi = 0;
    cpu.segments.ds = StoredSegment::real_mode(Segment::Ds, 0x1000);
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0x10000, &[0x56, 0x78]).unwrap();
    assert_eq!(machine.run_slice_until(1, |_| false).unwrap().completed, 1);
    machine.read_memory(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0x12, 0x34, 0x56, 0x78]);
}
