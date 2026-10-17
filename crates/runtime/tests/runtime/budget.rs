use super::machine;
use wasm86_x86::ExecutionExit;

#[pollster::test]
async fn instruction_budget_saturates_at_the_counter_limit() {
    let mut machine = machine(&[0x40, 0xeb, 0xfd]).await;
    let mut cpu = machine.cpu();
    cpu.instruction_count = u64::MAX - 4;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.run(10).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu().instruction_count, u64::MAX);
    assert_eq!((machine.cpu().registers.eax, machine.cpu().eip), (2, 0x100));
}

#[pollster::test]
async fn run_resumes_at_completed_block_boundaries() {
    let mut machine = machine(&[0x40, 0x43, 0xeb, 0xfc]).await;
    for iteration in 1..=3 {
        assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        let cpu = machine.cpu();
        assert_eq!(cpu.eip, 0x100);
        assert_eq!(
            (cpu.registers.eax, cpu.registers.ebx),
            (iteration, iteration)
        );
        assert_eq!(cpu.instruction_count, u64::from(iteration) * 3);
    }
}

#[pollster::test]
async fn zero_budget_does_not_enter_guest_code() {
    let mut machine = machine(&[0x0f, 0x0b]).await;
    let before = machine.cpu();
    assert_eq!(machine.run(0).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu(), before);
}
