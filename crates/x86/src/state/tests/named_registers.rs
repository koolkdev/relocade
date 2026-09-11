use super::super::{Cpu, State};
use crate::register::{Register, RegisterCode, RegisterOperand, RegisterType};
use crate::test_step::{
    Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule,
};
use crate::{CompiledModule, CpuState, Gpr32, Registers};
use wasm86_compiler::{Program, Signature, Type, I1, I16, I32, I64, I8};

fn initial_cpu() -> CpuState {
    let mut cpu = CpuState {
        registers: Registers {
            eax: 0x1020_3040,
            ecx: 0x5162_73ff,
            edx: 0x95a6_ffff,
            ebx: 0xd9ea_fb0c,
            esp: 0x1234_5678,
            ebp: 0x9abc_def0,
            esi: 0x1357_9bdf,
            edi: 0xffff_ffff,
        },
        eip: 0x1000,
        instruction_count: 0xffff_fffe,
        ..CpuState::filled(0xa5)
    };
    cpu.flags.kind = 9;
    cpu.flags.left = 7;
    cpu.flags.right = 8;
    cpu
}

fn assert_result(
    engine: Engine,
    module: &TestModule,
    initial: &CpuState,
    arguments: &[i32],
    expected: &CpuState,
    result: i64,
) {
    let input = Input {
        arguments: arguments.iter().copied().map(Argument::I32).collect(),
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        engine.observe(module, &input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(result)]),
                snapshot: Snapshot {
                    cpu: expected.to_bytes().to_vec(),
                    guest: None,
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "arguments {arguments:?}"
    );
}

fn named_views<T: RegisterType>() -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                for (parent, replacement) in [
                    (Gpr32::Eax, 0xa0b0_0010_u32),
                    (Gpr32::Ecx, 0xa1b1_0011),
                    (Gpr32::Edx, 0xa2b2_0012),
                    (Gpr32::Ebx, 0xa3b3_0013),
                    (Gpr32::Esp, 0xa4b4_0014),
                    (Gpr32::Ebp, 0xa5b5_0015),
                    (Gpr32::Esi, 0xa6b6_0016),
                    (Gpr32::Edi, 0xa7b7_0017),
                ] {
                    let old = state
                        .read_register(&mut body, RegisterOperand::Named(parent).view::<T>())?;
                    state.write_register(&mut body, parent, replacement)?;
                    state.write_register(&mut body, Register::<T>::named(parent), old.add(1))?;
                }
                state.publish(&mut body, 0x1004, 3)?;
                body.return_(7)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_named_views(engine: Engine) {
    let initial = initial_cpu();
    for (compiled, registers) in [
        (
            named_views::<I8>(),
            Registers {
                eax: 0xa0b0_0041,
                ecx: 0xa1b1_0000,
                edx: 0xa2b2_0000,
                ebx: 0xa3b3_000d,
                esp: 0xa4b4_0079,
                ebp: 0xa5b5_00f1,
                esi: 0xa6b6_00e0,
                edi: 0xa7b7_0000,
            },
        ),
        (
            named_views::<I16>(),
            Registers {
                eax: 0xa0b0_3041,
                ecx: 0xa1b1_7400,
                edx: 0xa2b2_0000,
                ebx: 0xa3b3_fb0d,
                esp: 0xa4b4_5679,
                ebp: 0xa5b5_def1,
                esi: 0xa6b6_9be0,
                edi: 0xa7b7_0000,
            },
        ),
        (
            named_views::<I32>(),
            Registers {
                eax: 0x1020_3041,
                ecx: 0x5162_7400,
                edx: 0x95a7_0000,
                ebx: 0xd9ea_fb0d,
                esp: 0x1234_5679,
                ebp: 0x9abc_def1,
                esi: 0x1357_9be0,
                edi: 0,
            },
        ),
    ] {
        let expected = CpuState {
            registers,
            eip: 0x1004,
            instruction_count: 1,
            ..initial
        };
        assert_result(
            engine,
            &TestModule::new(&compiled),
            &initial,
            &[],
            &expected,
            7,
        );
    }
}

#[test]
fn named_views_preserve_parent_identity_in_wasmtime() {
    check_named_views(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn named_views_preserve_parent_identity_in_v8() {
    check_named_views(Engine::V8);
}

fn mixed_byte_views() -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                results: vec![Type::I64],
            },
            |mut body| {
                let index = body.parameter::<I32>(0)?;
                let stop = body.parameter::<I1>(1)?;
                let mut state = State::new(&cpu);
                let esp_byte = RegisterOperand::Named(Gpr32::Esp).view::<I8>();
                let ah = RegisterOperand::Encoded(RegisterCode::from_code(4)).view::<I8>();
                let old_esp = state.read_register(&mut body, esp_byte.clone())?;
                let old_ah = state.read_register(&mut body, ah.clone())?;
                state.write_register(&mut body, Gpr32::Eax, 0x1122_3344)?;
                state.write_register(&mut body, Gpr32::Esp, 0x5566_7788)?;
                // A named ESP byte is an internal low-byte view; encoded byte 4 is AH.
                state.write_register(&mut body, Register::<I8>::named(Gpr32::Esp), 0x9a)?;
                state.write_register(&mut body, ah.clone(), 0xbc)?;
                body.if_(stop, |mut branch| {
                    state.publish(&mut branch, 0x1005, 2)?;
                    branch.return_(7)
                })?;
                let encoded = RegisterOperand::Encoded(RegisterCode::indexed(index.clone()));
                let before = state.read_register(&mut body, encoded.view::<I8>())?;
                state.write_register(
                    &mut body,
                    Register::<I8>::indexed(index),
                    old_esp.add(old_ah),
                )?;
                let high = state.read_register(&mut body, ah)?;
                let low = state.read_register(&mut body, esp_byte)?;
                state.write_register(
                    &mut body,
                    Register::<I16>::named(Gpr32::Edi),
                    high.unsigned()
                        .extend::<I16>()
                        .shl(8)
                        .or(low.unsigned().extend::<I16>()),
                )?;
                let eax = state.read_register(&mut body, Gpr32::Eax)?;
                state.publish(&mut body, 0x1009, 3)?;
                body.return_(
                    before
                        .unsigned()
                        .extend::<I64>()
                        .shl(32)
                        .or(eax.unsigned().extend::<I64>()),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_mixed_byte_views(engine: Engine) {
    let initial = initial_cpu();
    let module = TestModule::new(&mixed_byte_views());
    for (code, parent, value, before) in [
        (0, Gpr32::Eax, 0x1122_bca8, 0x44_u64),
        (1, Gpr32::Ecx, 0x5162_73a8, 0xff),
        (2, Gpr32::Edx, 0x95a6_ffa8, 0xff),
        (3, Gpr32::Ebx, 0xd9ea_fba8, 0x0c),
        (4, Gpr32::Eax, 0x1122_a844, 0xbc),
        (5, Gpr32::Ecx, 0x5162_a8ff, 0x73),
        (6, Gpr32::Edx, 0x95a6_a8ff, 0xff),
        (7, Gpr32::Ebx, 0xd9ea_a80c, 0xfb),
    ] {
        let mut expected = initial;
        expected.registers.eax = 0x1122_bc44;
        expected.registers.esp = 0x5566_779a;
        expected.registers[parent] = value;
        expected.registers.edi = if code == 4 { 0xffff_a89a } else { 0xffff_bc9a };
        expected.eip = 0x1009;
        expected.instruction_count = 1;
        assert_result(
            engine,
            &module,
            &initial,
            &[code, 0],
            &expected,
            ((before << 32) | u64::from(expected.registers.eax)) as i64,
        );
    }
    let mut expected = initial;
    expected.registers.eax = 0x1122_bc44;
    expected.registers.esp = 0x5566_779a;
    expected.eip = 0x1005;
    expected.instruction_count = 0;
    assert_result(engine, &module, &initial, &[4, 1], &expected, 7);
}

#[test]
fn named_and_encoded_byte_views_synchronize_in_wasmtime() {
    check_mixed_byte_views(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn named_and_encoded_byte_views_synchronize_in_v8() {
    check_mixed_byte_views(Engine::V8);
}
