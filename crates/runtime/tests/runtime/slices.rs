use super::machine;
use wasm86_runtime::{SliceOutcome, SliceStop, StepOutcome};
use wasm86_x86::{Exception, ExecutionExit, Segment, StoredSegment};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rep_yields_with_exact_work_and_step_finishes_the_remaining_instruction() {
    let mut machine = machine(&[0xf3, 0xa4, 0x40]).await;
    machine.write_memory(0x300, &[10, 20, 30, 40, 50]).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 5;
    cpu.registers.esi = 0x300;
    cpu.registers.edi = 0x400;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine.run_slice(2, |_| false).unwrap(),
        SliceOutcome {
            work: 2,
            completed: 0,
            stop: SliceStop::WorkLimit,
        }
    );
    let cpu = machine.cpu();
    assert_eq!(
        (
            cpu.eip,
            cpu.registers.ecx,
            cpu.registers.esi,
            cpu.registers.edi
        ),
        (0x100, 3, 0x302, 0x402)
    );
    let mut bytes = [0; 5];
    machine.read_memory(0x400, &mut bytes).unwrap();
    assert_eq!(bytes, [10, 20, 0, 0, 0]);
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    machine.read_memory(0x400, &mut bytes).unwrap();
    assert_eq!(bytes, [10, 20, 30, 40, 50]);
    assert_eq!(
        (machine.cpu().eip, machine.cpu().instruction_count),
        (0x102, 1)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn faults_report_completed_elements_without_retiring_rep() {
    let mut machine = machine(&[0x40, 0x67, 0xf3, 0xa5]).await;
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0xfffe;
    cpu.registers.edi = 0x300;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0xfffe, &[1, 2]).unwrap();
    assert_eq!(
        machine.run_slice(10, |_| false).unwrap(),
        SliceOutcome {
            work: 2,
            completed: 1,
            stop: SliceStop::Exited(ExecutionExit::Exception(Exception::GeneralProtection {
                error_code: 0
            })),
        }
    );
    assert_eq!((machine.cpu().eip, machine.cpu().registers.ecx), (0x101, 1));
    let mut bytes = [0; 4];
    machine.read_memory(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 0, 0]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn step_spans_rep_chunks_and_preserves_high_index_words_on_wrap() {
    let mut machine = machine(&[0xf3, 0xa4]).await;
    machine.write_memory(0xfffe, &[1, 2]).unwrap();
    machine.write_memory(0, &[3, 4]).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 0xabcd0004;
    cpu.registers.esi = 0x1234fffe;
    cpu.registers.edi = 0x56780300;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    let cpu = machine.cpu();
    assert_eq!(
        (cpu.registers.ecx, cpu.registers.esi, cpu.registers.edi),
        (0xabcd0000, 0x12340002, 0x56780304)
    );
    assert_eq!((cpu.eip, cpu.instruction_count), (0x102, 1));
    let mut bytes = [0; 4];
    machine.read_memory(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 3, 4]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn resumed_rep_observes_a20_at_the_next_element() {
    let mut machine = machine(&[0xf3, 0xa4]).await;
    machine.with_memory(|memory| {
        memory.write_backing(0xfffff, &[1, 2]).unwrap();
        memory.write_backing(0, &[3]).unwrap();
        memory.set_a20_enabled(false);
    });
    let mut cpu = machine.cpu();
    cpu.segments.ds = StoredSegment::real_mode(Segment::Ds, 0xffff);
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0xf;
    cpu.registers.edi = 0x300;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run_slice(1, |_| false).unwrap().completed, 0);
    machine.with_memory(|memory| memory.set_a20_enabled(true));
    assert_eq!(machine.run_slice(1, |_| false).unwrap().completed, 1);
    let mut bytes = [0; 2];
    machine.read_memory(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn empty_slices_and_host_stops_precede_fetch_and_work_limit() {
    let mut machine = machine(&[0x0f, 0x0b]).await;
    let before = machine.cpu();
    assert_eq!(
        machine.run_slice(0, |_| false).unwrap(),
        SliceOutcome {
            work: 0,
            completed: 0,
            stop: SliceStop::WorkLimit,
        }
    );
    assert_eq!(
        machine.run_slice(0, |_| true).unwrap().stop,
        SliceStop::HostRequested
    );
    assert_eq!(machine.cpu(), before);
    machine.write_memory(0x100, &[0x40]).unwrap();
    assert_eq!(
        machine
            .run_slice(1, |m| m.cpu().registers.eax == 1)
            .unwrap(),
        SliceOutcome {
            work: 1,
            completed: 1,
            stop: SliceStop::HostRequested,
        }
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn predicate_can_stop_at_partial_rep_boundary() {
    let mut machine = machine(&[0xf3, 0xa4]).await;
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0xfff;
    cpu.registers.edi = 0x300;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine
            .run_slice(10, |m| m.cpu().registers.ecx == 2)
            .unwrap(),
        SliceOutcome {
            work: 1,
            completed: 0,
            stop: SliceStop::HostRequested,
        }
    );
    assert_eq!(machine.cpu().eip, 0x100);
}
