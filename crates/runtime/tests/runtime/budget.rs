use super::machine;
use wasm86_x86::{Exception, ExecutionExit};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn instruction_budget_clamps_to_counter_range() {
    for observed in [false, true] {
        let mut machine = machine(&[0x40, 0xeb, 0xfd]).await; // INC AX; JMP 0100
        let mut cpu = machine.cpu();
        cpu.instruction_count = u64::MAX - 4;
        machine.set_cpu(cpu).unwrap();
        let outcome = if observed {
            machine.run_until(10, |_| false).unwrap().unwrap()
        } else {
            machine.run(10).unwrap()
        };
        assert_eq!(outcome, ExecutionExit::BudgetExhausted);
        let cpu = machine.cpu();
        assert_eq!(cpu.instruction_count, u64::MAX);
        assert_eq!((cpu.registers.eax, cpu.eip), (2, 0x100));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rep_completes_before_the_block_budget_check() {
    let mut machine = machine(&[0xf3, 0xa4, 0x40, 0xeb, 0]).await;
    let source: Vec<_> = (0..300).map(|i| (i % 251 + 1) as u8).collect();
    machine.write_memory(0x2000, &source).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 300;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    let cpu = machine.cpu();
    assert_eq!(
        (
            cpu.eip,
            cpu.registers.ecx,
            cpu.registers.esi,
            cpu.registers.edi
        ),
        (0x105, 0, 0x212c, 0x312c)
    );
    let mut bytes = vec![0; 300];
    machine.read_memory(0x3000, &mut bytes).unwrap();
    assert_eq!(bytes, source);
    assert_eq!(
        (machine.cpu().eip, machine.cpu().instruction_count),
        (0x105, 3)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn faulted_rep_preserves_effects_without_retiring() {
    let mut machine = machine(&[0x40, 0x67, 0xf3, 0xa5]).await;
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0xfffe;
    cpu.registers.edi = 0x300;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0xfffe, &[1, 2]).unwrap();
    assert_eq!(
        machine.run(10).unwrap(),
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!((machine.cpu().eip, machine.cpu().registers.ecx), (0x101, 1));
    assert_eq!(machine.cpu().instruction_count, 1);
    let mut bytes = [0; 4];
    machine.read_memory(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 0, 0]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rep_preserves_high_index_words_on_wrap() {
    let mut machine = machine(&[0xf3, 0xa4, 0xeb, 0]).await;
    machine.write_memory(0xfffe, &[1, 2]).unwrap();
    machine.write_memory(0, &[3, 4]).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 0xabcd0004;
    cpu.registers.esi = 0x1234fffe;
    cpu.registers.edi = 0x56780300;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    let cpu = machine.cpu();
    assert_eq!(
        (cpu.registers.ecx, cpu.registers.esi, cpu.registers.edi),
        (0xabcd0000, 0x12340002, 0x56780304)
    );
    assert_eq!((cpu.eip, cpu.instruction_count), (0x104, 2));
    let mut bytes = [0; 4];
    machine.read_memory(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 3, 4]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn zero_budget_and_observer_stops_precede_fetch() {
    let mut machine = machine(&[0x0f, 0x0b]).await;
    let before = machine.cpu();
    assert_eq!(machine.run(0).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.run_until(0, |_| true).unwrap(), None);
    assert_eq!(machine.cpu(), before);
    machine.write_memory(0x100, &[0x40, 0xeb, 0]).unwrap();
    assert_eq!(
        machine
            .run_until(1, |m| m.cpu().registers.eax == 1)
            .unwrap(),
        None
    );
}
