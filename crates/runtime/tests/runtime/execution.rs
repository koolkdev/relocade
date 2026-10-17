use super::machine;
use wasm86_x86::{Exception, ExecutionExit};

#[pollster::test]
async fn persistent_state_and_independent_machines() {
    let mut first = machine(&[0xb8, 0x34, 0x12, 0xa3, 0, 2, 0x40, 0xeb, 0, 0x0f, 0x0b]).await;
    let second = machine(&[0x90]).await;
    assert_eq!(first.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(first.cpu().registers.eax, 0x1235);
    assert_eq!(first.cpu().instruction_count, 4);
    assert_eq!(second.cpu().registers.eax, 0);
    assert_eq!(second.cpu().instruction_count, 0);
    let before = first.cpu();
    assert_eq!(
        first.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(first.cpu(), before);
}

#[pollster::test]
async fn guest_code_changes_are_visible_at_the_next_instruction() {
    // MOV byte [0105],40; NOP (replaced with INC AX); JMP to the next block.
    let mut machine = machine(&[0xc6, 0x06, 0x05, 0x01, 0x40, 0x90, 0xeb, 0]).await;
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu().registers.eax, 1);
    assert_eq!(machine.cpu().eip, 0x108);
}

#[pollster::test]
async fn unsupported_instruction_is_distinct_from_an_architectural_exception() {
    let mut machine = machine(&[0xf4]).await; // HLT is outside the current subset.
    assert_eq!(
        machine.run(1).unwrap(),
        ExecutionExit::UnsupportedInstruction {
            eip: 0x100,
            opcode: 0xf4
        }
    );
    assert_eq!(machine.cpu().eip, 0x100);
    assert_eq!(machine.cpu().instruction_count, 0);
}

#[pollster::test]
async fn incompatible_cpu_replacement_leaves_the_machine_unchanged() {
    let mut machine = machine(&[0x90, 0xeb, 0]).await;
    let before = machine.cpu();
    let mut invalid = before;
    invalid.segments.cs.base = 1;
    assert!(machine.set_cpu(invalid).is_err());
    assert_eq!(machine.cpu(), before);
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
}
