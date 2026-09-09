use super::super::{Cpu, Register, State};
use wasm86_compiler::{Program, Signature, Type, I1, I16, I32};

use crate::test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};

use wasm86_compiler::{I64, I8};

use crate::register::RegisterCode;
use crate::{CpuState, Gpr32, Registers};

enum IndexSource {
    Parameter,
    OldEax,
}

fn synchronized_registers(source: IndexSource) -> crate::CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program.declare(Signature {
        parameters: vec![Type::I32, Type::I1],
        result: Some(Type::I64),
    });
    let mut body = program.define(function).unwrap();
    let mut state = State::new(&cpu);
    let index = match source {
        IndexSource::Parameter => body.parameter::<I32>(0).unwrap(),
        IndexSource::OldEax => state.read_register(&mut body, Gpr32::Eax).unwrap(),
    };
    let stop = body.parameter::<I1>(1).unwrap();
    state.write_register(&mut body, Gpr32::Eax, 42).unwrap();
    body.if_(stop, |mut branch| {
        state.publish(&mut branch, 0x1005, 1)?;
        branch.return_(7)
    })
    .unwrap();
    let before = state
        .read_register(&mut body, Register::<I32>::indexed(index.clone()))
        .unwrap();
    state
        .write_register(&mut body, Register::<I32>::indexed(index), 99)
        .unwrap();
    let after = state.read_register(&mut body, Gpr32::Eax).unwrap();
    state.publish(&mut body, 0x100a, 2).unwrap();
    body.return_(
        before
            .unsigned()
            .extend::<I64>()
            .shl(32)
            .or(after.unsigned().extend::<I64>()),
    )
    .unwrap();
    program.export("run", function).unwrap();
    crate::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_register_observation(
    module: &TestModule,
    initial: &CpuState,
    arguments: [u32; 2],
    expected_cpu: &CpuState,
    result: i64,
) {
    let [index, stop] = arguments;
    let input = Input {
        arguments: vec![Argument::I32(index as i32), Argument::I32(stop as i32)],
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        module.observe(&input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(Some(Argument::I64(result))),
                snapshot: Snapshot {
                    cpu: expected_cpu.to_bytes().to_vec(),
                    guest: None,
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "index {index}, stop {stop}"
    );
}

#[test]
fn register_synchronization_in_wasmtime() {
    let mut initial = CpuState {
        registers: Registers {
            eax: 0x1111_1111,
            ecx: 0x2222_2222,
            edx: 0x3333_3333,
            ebx: 0x4444_4444,
            esp: 0x5555_5555,
            ebp: 0x6666_6666,
            esi: 0x7777_7777,
            edi: 0x8888_8888,
        },
        eip: 0x1000,
        instruction_count: 0xffff_ffff,
        reserved_tail: [0; 4],
        ..CpuState::filled(0xa5)
    };
    let module = TestModule::new(&synchronized_registers(IndexSource::Parameter));
    for (index, register, result) in [
        (0, Gpr32::Eax, 0x0000_002a_0000_0063_u64),
        (1, Gpr32::Ecx, 0x2222_2222_0000_002a),
        (2, Gpr32::Edx, 0x3333_3333_0000_002a),
        (3, Gpr32::Ebx, 0x4444_4444_0000_002a),
        (4, Gpr32::Esp, 0x5555_5555_0000_002a),
        (5, Gpr32::Ebp, 0x6666_6666_0000_002a),
        (6, Gpr32::Esi, 0x7777_7777_0000_002a),
        (7, Gpr32::Edi, 0x8888_8888_0000_002a),
    ] {
        let mut expected = initial;
        expected.registers.eax = 42;
        expected.registers[register] = 99;
        expected.eip = 0x100a;
        expected.instruction_count = 1;
        check_register_observation(&module, &initial, [index, 0], &expected, result as i64);
    }
    let mut expected = initial;
    expected.registers.eax = 42;
    expected.eip = 0x1005;
    expected.instruction_count = 0;
    check_register_observation(&module, &initial, [0, 1], &expected, 7);

    initial.registers.eax = 5;
    let module = TestModule::new(&synchronized_registers(IndexSource::OldEax));
    let mut expected = initial;
    expected.registers.eax = 42;
    expected.registers.ebp = 99;
    expected.eip = 0x100a;
    expected.instruction_count = 1;
    check_register_observation(&module, &initial, [0, 0], &expected, 0x6666_6666_0000_002a);
}

fn synchronized_byte_registers() -> crate::CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                result: Some(Type::I64),
            },
            |mut body| {
                let index = body.parameter::<I32>(0)?;
                let stop = body.parameter::<I1>(1)?;
                let mut state = State::new(&cpu);
                let old_high =
                    state.read_register(&mut body, RegisterCode::from_code(4).view::<I8>())?;
                state.write_register(&mut body, Gpr32::Eax, 0x1122_3344)?;
                state.write_register(&mut body, Gpr32::Esp, 0x1357_9bdf)?;
                state.write_register(&mut body, RegisterCode::from_code(4).view::<I8>(), 0xaa)?;
                body.if_(stop, |mut branch| {
                    state.publish(&mut branch, 0x1007, 2)?;
                    branch.return_(7)
                })?;
                let before =
                    state.read_register(&mut body, Register::<I8>::indexed(index.clone()))?;
                state.write_register(&mut body, Register::<I8>::indexed(index), old_high.add(1))?;
                let after = state.read_register(&mut body, Gpr32::Eax)?;
                state.publish(&mut body, 0x1009, 3)?;
                body.return_(
                    before
                        .unsigned()
                        .extend::<I64>()
                        .shl(32)
                        .or(after.unsigned().extend::<I64>()),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    crate::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

#[test]
fn byte_register_synchronization_in_wasmtime() {
    let initial = CpuState {
        registers: Registers {
            eax: 0xffff_ff04,
            ecx: 0x2222_2222,
            edx: 0x3333_3333,
            ebx: 0x4444_4444,
            esp: 0x5555_5555,
            ebp: 0x6666_6666,
            esi: 0x7777_7777,
            edi: 0x8888_8888,
        },
        eip: 0x1000,
        instruction_count: 0xffff_ffff,
        ..CpuState::filled(0xa5)
    };
    let module = TestModule::new(&synchronized_byte_registers());
    for (index, parent, parent_value, before, eax) in [
        (0, Gpr32::Eax, 0x1122_aa00, 0x44_u64, 0x1122_aa00_u32),
        (1, Gpr32::Ecx, 0x2222_2200, 0x22, 0x1122_aa44),
        (2, Gpr32::Edx, 0x3333_3300, 0x33, 0x1122_aa44),
        (3, Gpr32::Ebx, 0x4444_4400, 0x44, 0x1122_aa44),
        (4, Gpr32::Eax, 0x1122_0044, 0xaa, 0x1122_0044),
        (5, Gpr32::Ecx, 0x2222_0022, 0x22, 0x1122_aa44),
        (6, Gpr32::Edx, 0x3333_0033, 0x33, 0x1122_aa44),
        (7, Gpr32::Ebx, 0x4444_0044, 0x44, 0x1122_aa44),
    ] {
        let mut expected = initial;
        expected.registers.eax = eax;
        expected.registers.esp = 0x1357_9bdf;
        expected.registers[parent] = parent_value;
        expected.eip = 0x1009;
        expected.instruction_count = 2;
        check_register_observation(
            &module,
            &initial,
            [index, 0],
            &expected,
            ((before << 32) | u64::from(eax)) as i64,
        );
    }
    let mut expected = initial;
    expected.registers.eax = 0x1122_aa44;
    expected.registers.esp = 0x1357_9bdf;
    expected.eip = 0x1007;
    expected.instruction_count = 1;
    check_register_observation(&module, &initial, [4, 1], &expected, 7);
}

fn synchronized_word_registers() -> crate::CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                result: Some(Type::I64),
            },
            |mut body| {
                let index = body.parameter::<I32>(0)?;
                let stop = body.parameter::<I1>(1)?;
                let mut state = State::new(&cpu);
                let old_word =
                    state.read_register(&mut body, RegisterCode::from_code(0).view::<I16>())?;
                state.write_register(&mut body, Gpr32::Eax, 0x1122_3344)?;
                state.write_register(&mut body, Gpr32::Edi, 0x1357_9bdf)?;
                state.write_register(
                    &mut body,
                    RegisterCode::from_code(0).view::<I16>(),
                    old_word.add(1),
                )?;
                state.write_register(&mut body, RegisterCode::from_code(4).view::<I8>(), 0xaa)?;
                body.if_(stop, |mut branch| {
                    state.publish(&mut branch, 0x1009, 4)?;
                    branch.return_(7)
                })?;
                let before =
                    state.read_register(&mut body, Register::<I16>::indexed(index.clone()))?;
                state.write_register(
                    &mut body,
                    Register::<I16>::indexed(index),
                    old_word.add(0x3323),
                )?;
                let after = state.read_register(&mut body, Gpr32::Eax)?;
                state.publish(&mut body, 0x100b, 5)?;
                body.return_(
                    before
                        .unsigned()
                        .extend::<I64>()
                        .shl(32)
                        .or(after.unsigned().extend::<I64>()),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    crate::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

#[test]
fn word_register_synchronization_in_wasmtime() {
    let initial = CpuState {
        registers: Registers {
            eax: 0xaaaa_ccdd,
            ecx: 0x2222_2222,
            edx: 0x3333_3333,
            ebx: 0x4444_4444,
            esp: 0x5555_5555,
            ebp: 0x6666_6666,
            esi: 0x7777_7777,
            edi: 0x8888_8888,
        },
        eip: 0x1000,
        instruction_count: 0xffff_ffff,
        ..CpuState::filled(0xa5)
    };
    let module = TestModule::new(&synchronized_word_registers());
    for (index, parent, parent_value, before, eax) in [
        (0, Gpr32::Eax, 0x1122_0000, 0xaade_u64, 0x1122_0000_u32),
        (1, Gpr32::Ecx, 0x2222_0000, 0x2222, 0x1122_aade),
        (2, Gpr32::Edx, 0x3333_0000, 0x3333, 0x1122_aade),
        (3, Gpr32::Ebx, 0x4444_0000, 0x4444, 0x1122_aade),
        (4, Gpr32::Esp, 0x5555_0000, 0x5555, 0x1122_aade),
        (5, Gpr32::Ebp, 0x6666_0000, 0x6666, 0x1122_aade),
        (6, Gpr32::Esi, 0x7777_0000, 0x7777, 0x1122_aade),
        (7, Gpr32::Edi, 0x1357_0000, 0x9bdf, 0x1122_aade),
    ] {
        let mut expected = initial;
        expected.registers.eax = eax;
        expected.registers.edi = 0x1357_9bdf;
        // The held 0xccdd plus 0x3323 wraps to zero in the word store.
        expected.registers[parent] = parent_value;
        expected.eip = 0x100b;
        expected.instruction_count = 4;
        check_register_observation(
            &module,
            &initial,
            [index, 0],
            &expected,
            ((before << 32) | u64::from(eax)) as i64,
        );
    }
    let mut expected = initial;
    expected.registers.eax = 0x1122_aade;
    expected.registers.edi = 0x1357_9bdf;
    expected.eip = 0x1009;
    expected.instruction_count = 3;
    check_register_observation(&module, &initial, [7, 1], &expected, 7);
}
