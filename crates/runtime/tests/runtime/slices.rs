use super::machine;
use wasm86_runtime::{SliceOutcome, SliceStop};
use wasm86_x86::{Exception, ExecutionExit};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rep_completes_before_the_slice_returns() {
    let mut machine = machine(&[0xf3, 0xa4, 0x40]).await;
    let source: Vec<_> = (0..300).map(|i| (i % 251 + 1) as u8).collect();
    machine.write_memory(0x2000, &source).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 300;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine.run_slice(1).unwrap(),
        SliceOutcome {
            work: 1,
            completed: 1,
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
        (0x102, 0, 0x212c, 0x312c)
    );
    let mut bytes = vec![0; 300];
    machine.read_memory(0x3000, &mut bytes).unwrap();
    assert_eq!(bytes, source);
    assert_eq!(
        (machine.cpu().eip, machine.cpu().instruction_count),
        (0x102, 1)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn faulted_rep_preserves_effects_without_consuming_instruction_work() {
    let mut machine = machine(&[0x40, 0x67, 0xf3, 0xa5]).await;
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0xfffe;
    cpu.registers.edi = 0x300;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0xfffe, &[1, 2]).unwrap();
    assert_eq!(
        machine.run_slice(10).unwrap(),
        SliceOutcome {
            work: 1,
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
async fn rep_preserves_high_index_words_on_wrap() {
    let mut machine = machine(&[0xf3, 0xa4]).await;
    machine.write_memory(0xfffe, &[1, 2]).unwrap();
    machine.write_memory(0, &[3, 4]).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 0xabcd0004;
    cpu.registers.esi = 0x1234fffe;
    cpu.registers.edi = 0x56780300;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run_slice(1).unwrap().completed, 1);
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
async fn empty_slices_and_host_stops_precede_fetch_and_work_limit() {
    let mut machine = machine(&[0x0f, 0x0b]).await;
    let before = machine.cpu();
    assert_eq!(
        machine.run_slice(0).unwrap(),
        SliceOutcome {
            work: 0,
            completed: 0,
            stop: SliceStop::WorkLimit,
        }
    );
    assert_eq!(
        machine.run_slice_until(0, |_| true).unwrap().stop,
        SliceStop::HostRequested
    );
    assert_eq!(machine.cpu(), before);
    machine.write_memory(0x100, &[0x40]).unwrap();
    assert_eq!(
        machine
            .run_slice_until(1, |m| m.cpu().registers.eax == 1)
            .unwrap(),
        SliceOutcome {
            work: 1,
            completed: 1,
            stop: SliceStop::HostRequested,
        }
    );
}
