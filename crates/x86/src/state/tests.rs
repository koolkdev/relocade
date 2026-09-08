use super::{declare, Gpr32, Register, State};
use wasm86_compiler::{Program, Signature, Type, I1, I32};
use wasmparser::{Operator, Parser, Payload};

#[test]
fn named_writes_coalesce_without_crossing_indexed_writes() {
    let mut program = Program::new();
    let memory = declare(&mut program);
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    let index = body.parameter::<I32>(0).unwrap();
    let mut state = State::new(memory);
    state.write_register(&mut body, Gpr32::Eax, 0).unwrap();
    state.write_register(&mut body, Gpr32::Eax, 1).unwrap();
    state
        .write_register(&mut body, Register::<I32>::indexed(index), 2)
        .unwrap();
    state.write_register(&mut body, Gpr32::Eax, 3).unwrap();
    state.write_register(&mut body, Gpr32::Eax, 4).unwrap();
    state.publish(&mut body, 7, 1).unwrap();
    body.return_(0).unwrap();
    let bytes = program.compile().unwrap();
    let mut stored = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(code) = payload.unwrap() {
            let mut previous_constant = None;
            for operation in code.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                if matches!(operation, Operator::I32Store { memarg } if memarg.offset == 24) {
                    stored.push(previous_constant.expect("register store has its literal value"));
                }
                previous_constant = match operation {
                    Operator::I32Const { value } => Some(value),
                    _ => None,
                };
            }
        }
    }
    // Index zero names EAX too: the last write must remain after the indexed one.
    assert_eq!(stored, [1, 2, 4]);
}

#[test]
fn publishing_an_exit_keeps_pending_writes_for_the_continuation() {
    let mut program = Program::new();
    let memory = declare(&mut program);
    let function = program.declare(Signature {
        parameters: vec![Type::I1],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    let stop = body.parameter::<I1>(0).unwrap();
    let mut state = State::new(memory);
    state.write_register(&mut body, Gpr32::Eax, 42).unwrap();
    body.if_(stop, |mut branch| {
        state.publish(&mut branch, 0x1005, 1)?;
        branch.return_(-1)
    })
    .unwrap();
    state.write_register(&mut body, Gpr32::Eax, 43).unwrap();
    state.write_register(&mut body, Gpr32::Ecx, 99).unwrap();
    state.publish(&mut body, 0x100a, 2).unwrap();
    body.return_(0).unwrap();
    program.export("run", function).unwrap();

    let bytes = program.compile().unwrap();
    wasmparser::Validator::new().validate_all(&bytes).unwrap();
    let mut depth = 0;
    let mut exit_stores = Vec::new();
    let mut continuation_stores = Vec::new();
    let mut eax_values = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(code) = payload.unwrap() {
            let mut previous_constant = None;
            for operation in code.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                if matches!(operation, Operator::I32Store { memarg } if memarg.offset == 24) {
                    eax_values.push((depth, previous_constant.expect("EAX has a literal value")));
                }
                previous_constant = match operation {
                    Operator::I32Const { value } => Some(value),
                    _ => None,
                };
                match operation {
                    Operator::If { .. } => depth += 1,
                    Operator::End if depth > 0 => depth -= 1,
                    Operator::I32Store { memarg } if depth > 0 => {
                        exit_stores.push(memarg.offset);
                    }
                    Operator::I32Store { memarg } => continuation_stores.push(memarg.offset),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(exit_stores, [24, 56, 144]);
    assert_eq!(continuation_stores, [24, 28, 56, 144]);
    assert_eq!(eax_values, [(1, 42), (0, 43)]);
}

use crate::test_step::ModuleFile;

use std::fmt::Write as _;
use wasm86_compiler::{I64, I8};

use crate::register::RegisterCode;

enum IndexSource {
    Parameter,
    OldEax,
}

fn synchronized_registers(source: IndexSource) -> crate::CompiledModule {
    let mut program = Program::new();
    let memory = declare(&mut program);
    let function = program.declare(Signature {
        parameters: vec![Type::I32, Type::I1],
        result: Type::I64,
    });
    let mut body = program.define(function).unwrap();
    let mut state = State::new(memory);
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
    module: &ModuleFile,
    flags: &[&str],
    initial: &[u8; 152],
    arguments: [u32; 2],
    expected_cpu: &[u8; 152],
    result: i64,
) {
    let mut expected = format!("return {result}\nstate ");
    for byte in expected_cpu {
        write!(&mut expected, "{byte:02x}").unwrap();
    }
    expected.push_str("\nguest unchanged\nmachine unchanged\n");
    let [index, stop] = arguments;
    let input = format!("[{initial:?},[],[],[[\"i32\",{index}],[\"i32\",{stop}]]]");
    assert_eq!(
        module.observe(flags, &input, 1),
        expected,
        "index {index}, stop {stop}, flags {flags:?}"
    );
}

fn check_register_synchronization(flags: &[&str]) {
    let mut initial = [0xa5; 152];
    for (offset, value) in [
        (24, 0x1111_1111_u32),
        (28, 0x2222_2222),
        (32, 0x3333_3333),
        (36, 0x4444_4444),
        (40, 0x5555_5555),
        (44, 0x6666_6666),
        (48, 0x7777_7777),
        (52, 0x8888_8888),
        (56, 0x1000),
        (144, 0xffff_ffff),
        (148, 0),
    ] {
        initial[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let check = |module: &ModuleFile,
                 cpu: &[u8; 152],
                 index: u32,
                 stop: u32,
                 updates: &[(usize, u32)],
                 result: i64| {
        let mut expected_cpu = *cpu;
        for &(offset, value) in updates {
            expected_cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        check_register_observation(module, flags, cpu, [index, stop], &expected_cpu, result);
    };
    let module = ModuleFile::new(&synchronized_registers(IndexSource::Parameter));
    for (index, offset, result) in [
        (0, 24, 0x0000_002a_0000_0063_u64),
        (1, 28, 0x2222_2222_0000_002a),
        (2, 32, 0x3333_3333_0000_002a),
        (3, 36, 0x4444_4444_0000_002a),
        (4, 40, 0x5555_5555_0000_002a),
        (5, 44, 0x6666_6666_0000_002a),
        (6, 48, 0x7777_7777_0000_002a),
        (7, 52, 0x8888_8888_0000_002a),
    ] {
        check(
            &module,
            &initial,
            index,
            0,
            &[(24, 42), (offset, 99), (56, 0x100a), (144, 1)],
            result as i64,
        );
    }
    check(
        &module,
        &initial,
        0,
        1,
        &[(24, 42), (56, 0x1005), (144, 0)],
        7,
    );
    initial[24..28].copy_from_slice(&5_u32.to_le_bytes());
    let module = ModuleFile::new(&synchronized_registers(IndexSource::OldEax));
    check(
        &module,
        &initial,
        0,
        0,
        &[(24, 42), (44, 99), (56, 0x100a), (144, 1)],
        0x6666_6666_0000_002a,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn register_synchronization_in_v8() {
    check_register_synchronization(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn register_synchronization_in_optimizing_v8() {
    check_register_synchronization(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}

fn synchronized_byte_registers() -> crate::CompiledModule {
    let mut program = Program::new();
    let memory = declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                result: Type::I64,
            },
            |mut body| {
                let index = body.parameter::<I32>(0)?;
                let stop = body.parameter::<I1>(1)?;
                let mut state = State::new(memory);
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

fn check_byte_register_synchronization(flags: &[&str]) {
    let mut initial = [0xa5; 152];
    for (offset, value) in [
        (24, 0xffff_ff04_u32),
        (28, 0x2222_2222),
        (32, 0x3333_3333),
        (36, 0x4444_4444),
        (40, 0x5555_5555),
        (44, 0x6666_6666),
        (48, 0x7777_7777),
        (52, 0x8888_8888),
        (56, 0x1000),
        (144, 0xffff_ffff),
    ] {
        initial[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let module = ModuleFile::new(&synchronized_byte_registers());
    for (index, offset, before, eax) in [
        (0, 24, 0x44_u64, 0x1122_aa00_u32),
        (1, 28, 0x22, 0x1122_aa44),
        (2, 32, 0x33, 0x1122_aa44),
        (3, 36, 0x44, 0x1122_aa44),
        (4, 25, 0xaa, 0x1122_0044),
        (5, 29, 0x22, 0x1122_aa44),
        (6, 33, 0x33, 0x1122_aa44),
        (7, 37, 0x44, 0x1122_aa44),
    ] {
        let mut expected = initial;
        for (offset, value) in [(24, eax), (40, 0x1357_9bdf), (56, 0x1009), (144, 2)] {
            expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        expected[offset] = 0;
        check_register_observation(
            &module,
            flags,
            &initial,
            [index, 0],
            &expected,
            ((before << 32) | u64::from(eax)) as i64,
        );
    }
    let mut expected = initial;
    for (offset, value) in [
        (24, 0x1122_aa44_u32),
        (40, 0x1357_9bdf),
        (56, 0x1007),
        (144, 1),
    ] {
        expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    check_register_observation(&module, flags, &initial, [4, 1], &expected, 7);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn byte_register_synchronization_in_v8() {
    check_byte_register_synchronization(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn byte_register_synchronization_in_optimizing_v8() {
    check_byte_register_synchronization(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
