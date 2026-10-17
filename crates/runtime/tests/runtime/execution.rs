use super::machine;
use wasm86_x86::{Exception, ExecutionExit, Segment, StoredSegment};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn persistent_state_and_independent_machines() {
    let mut first = machine(&[0xb8, 0x34, 0x12, 0xa3, 0, 2, 0x40, 0xeb, 0, 0x0f, 0x0b]).await;
    let second = machine(&[0x90]).await;
    assert_eq!(first.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(first.cpu().registers.eax, 0x1235);
    assert_eq!(first.cpu().instruction_count, 4);
    let mut bytes = [0; 2];
    first.read_memory(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0x34, 0x12]);
    assert_eq!(second.cpu().registers.eax, 0);
    assert_eq!(second.cpu().instruction_count, 0);
    let before = first.cpu();
    assert_eq!(
        first.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(first.cpu(), before);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn guest_code_changes_are_visible_at_the_next_instruction() {
    // MOV byte [0105],40; NOP (replaced with INC AX); JMP to the next block.
    let mut machine = machine(&[0xc6, 0x06, 0x05, 0x01, 0x40, 0x90, 0xeb, 0]).await;
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu().registers.eax, 1);
    assert_eq!(machine.cpu().eip, 0x108);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
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

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn interrupt_handler_returns_through_the_saved_frame() {
    let mut machine = machine(&[]).await;
    let mut cpu = machine.cpu();
    cpu.segments.cs = StoredSegment::real_mode(Segment::Cs, 0x1000);
    cpu.flags.bytes.if_ = 1;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0x10100, &[0xcd, 0x21, 0x90]).unwrap();
    machine.write_memory(0x84, &[0x10, 0, 0x34, 0x12]).unwrap();
    machine
        .write_memory(0x12350, &[0xb8, 0xef, 0xbe, 0xcf])
        .unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(machine.cpu().segments.cs.selector, 0x1234);
    assert_eq!(machine.cpu().eip, 0x10);
    assert_eq!(machine.cpu().registers.esp, 0xfefa);
    let mut frame = [0; 6];
    machine.read_memory(0xfefa, &mut frame).unwrap();
    assert_eq!(frame, [2, 1, 0, 0x10, 2, 2]);
    // A host service can return CF through the saved FLAGS without decoding lazy flags.
    machine.write_memory(0xfefe, &[3, 2]).unwrap();
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    let cpu = machine.cpu();
    assert_eq!((cpu.segments.cs.selector, cpu.eip), (0x1000, 0x102));
    assert_eq!(cpu.registers.esp, 0xff00);
    assert_eq!(cpu.registers.eax, 0xbeef);
    assert_eq!(cpu.flags.bytes.cf & 1, 1);
    assert_eq!(cpu.flags.bytes.if_ & 1, 1);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn incompatible_cpu_replacement_leaves_the_machine_unchanged() {
    let mut machine = machine(&[0x90, 0xeb, 0]).await;
    let before = machine.cpu();
    let mut invalid = before;
    invalid.segments.cs.base = 1;
    assert!(machine.set_cpu(invalid).is_err());
    assert_eq!(machine.cpu(), before);
    assert_eq!(machine.run(1).unwrap(), ExecutionExit::BudgetExhausted);
}
