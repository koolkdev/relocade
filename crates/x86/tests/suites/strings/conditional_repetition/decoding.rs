use super::*;
use crate::support::{
    cases::{test_cases, Permissions::ReadOnly},
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ecx, Edi, Esi},
};

fn duplicate_prefixes() -> Vec<Case> {
    let mut cases = Vec::new();
    for prefixes in [
        &[0xf2, 0xf3][..],
        &[0xf3, 0xf2],
        &[0xf2, 0xf2],
        &[0xf3, 0xf3],
        &[0x66, 0xf3, 0x67, 0xf2],
        &[0xf2, 0x66, 0xf3, 0x67],
    ] {
        let selected = *prefixes
            .iter()
            .rfind(|&&byte| byte == 0xf2 || byte == 0xf3)
            .unwrap();
        let consumed = if selected == 0xf3 { 2 } else { 1 };
        for operation in COMPARISONS {
            let opcode = if operation == Operation::Cmps {
                0xa6
            } else {
                0xae
            };
            let code = [prefixes, &[opcode]].concat();
            let mut case = Case::replacing_flags(
                format!("last F2/F3 decoder policy {code:02x?}"),
                &code,
                flags(if selected == 0xf3 { 0 } else { 10 }),
            )
            .stored_flags(record(0xfe))
            .register(Ecx, 3, 3 - consumed)
            .register(Edi, 0x7000, 0x7000 + consumed)
            .initial_register(Eax, 5)
            .memory(0x7000, &[5, 3, 5], ReadOnly);
            if operation == Operation::Cmps {
                case = case
                    .register(Esi, 0x4000, 0x4000 + consumed)
                    .memory(0x4000, &[5; 3], ReadOnly);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    last_repeat_prefix_selects_the_comparison_condition,
    duplicate_prefixes()
);

#[test]
fn snapshot_fetch_lengths_and_successors_after_repetition() {
    for prefix in [0xf2, 0xf3] {
        for opcode in [0xa6, 0xa7, 0xae, 0xaf] {
            for length in [1, 2, 14, 15] {
                let mut code = vec![prefix; length];
                code.push(opcode);
                for available in 0..code.len() {
                    assert_eq!(
                        compile_block_from_bytes(0x1000, &code[..available], 1).err(),
                        Some(if available >= 15 {
                            BlockError::InstructionTooLong { address: 0x1000 }
                        } else {
                            BlockError::TruncatedInstruction {
                                address: 0x1000,
                                available,
                            }
                        })
                    );
                }
                if length == 15 {
                    assert_eq!(
                        compile_block_from_bytes(0x1000, &code, 1).err(),
                        Some(BlockError::InstructionTooLong { address: 0x1000 })
                    );
                } else {
                    let module = compile_block_from_bytes(0x1000, &code, 1).unwrap();
                    let next_eip = 0x1000 + code.len() as u32;
                    code.push(0x0f);
                    assert_eq!(
                        module.bytes,
                        compile_block_from_bytes(0x1000, &code, 1).unwrap().bytes
                    );
                    assert_eq!(
                        compile_block_from_bytes(0x1000, &code, 2).err(),
                        Some(BlockError::TruncatedInstruction {
                            address: next_eip,
                            available: 1
                        })
                    );
                }
            }
        }
    }
}

fn fetch(engine: Engine) {
    for prefix in [0xf2, 0xf3] {
        for length in [1, 14, 15] {
            let mut image = Image::new(&[]);
            image.cpu.eip = 0x2000 - length;
            image.cpu.registers.ecx = 0;
            image.data(0x4000 - length, &vec![prefix; length as usize]);
            let exit = if length == 15 {
                Exit::Other(0x0002_0000_0000_0000)
            } else {
                Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                }
            };
            assert_eq!(
                engine.observe(TestModule::interpreter(), &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit
                    }]
                )
            );
        }
        for opcode in [0x90, 0x0f, 0xac] {
            let mut image = Image::new(&[]);
            image.cpu.eip = 0x1ffe;
            image.cpu.registers.ecx = 0;
            image.data(0x3ffe, &[prefix, opcode]);
            assert_eq!(
                engine.observe(TestModule::interpreter(), &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::Other(0x0008_0000_0000_1ffe | (u64::from(prefix) << 32))
                    }]
                )
            );
        }
    }
    for prefix in [0xf2, 0xf3] {
        use crate::support::guest::Machine;
        let mut machine = Machine::new(&[prefix, 0xae, 0xae]);
        machine.cpu.flags = record(0xfe);
        machine.cpu.registers.eax = 5;
        machine.cpu.registers.ecx = 3;
        machine.cpu.registers.edi = 0x7000;
        machine.memory(
            0x7000,
            if prefix == 0xf3 {
                &[3, 5, 3]
            } else {
                &[5, 3, 5]
            },
            ReadOnly,
        );
        let steps = machine.run_many(TestModule::interpreter(), engine, 2);
        for (index, step) in steps.iter().enumerate() {
            assert_eq!(
                step.state.cpu.registers.ecx, 2,
                "repeat state must reset before the next SCAS"
            );
            assert_eq!(step.state.cpu.registers.edi, 0x7001 + index as u32);
            assert_eq!(step.exit, Exit::Dispatch(0x1002 + index as u32));
        }
    }
}

#[test]
fn prefix_fetch_and_unsupported_forms_precede_operand_access() {
    fetch(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn prefix_fetch_and_unsupported_forms_precede_operand_access_v8() {
    fetch(Engine::V8);
}
