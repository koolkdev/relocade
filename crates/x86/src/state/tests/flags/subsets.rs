use crate::flags::{FlagMask, StatusFlag};
use crate::state::access::cpu_load;
use crate::state::Cpu;
use crate::test_step::{Argument, CallPatches, Event, Input, Outcome, TestModule};
use crate::FlagBytes;
use crate::{CompiledModule, CpuState};
use wasm86_compiler::{Program, Signature, Type, Val, I1, I64};
use wasmparser::{Operator, Parser, Payload, ValType, Validator};

fn mask(bits: u8) -> FlagMask {
    [
        StatusFlag::CF,
        StatusFlag::PF,
        StatusFlag::AF,
        StatusFlag::ZF,
        StatusFlag::SF,
        StatusFlag::OF,
    ]
    .into_iter()
    .enumerate()
    .fold(FlagMask::EMPTY, |mask, (index, flag)| {
        if bits & (1 << index) != 0 {
            mask.union(FlagMask::of(flag))
        } else {
            mask
        }
    })
}

fn status_image(status: [bool; 6]) -> u8 {
    status
        .into_iter()
        .enumerate()
        .fold(0, |bits, (index, value)| bits | (u8::from(value) << index))
}

fn arithmetic_flags(width: u32, add: bool, left: u32, right: u32) -> u8 {
    let modulus = 2_u64.pow(width);
    let sign = modulus / 2;
    let left = u64::from(left);
    let right = u64::from(right);
    let signed = |value: u64| {
        if value >= sign {
            value as i64 - modulus as i64
        } else {
            value as i64
        }
    };
    let (result, carry, auxiliary, mathematical) = if add {
        (
            (left + right) % modulus,
            left + right >= modulus,
            left % 16 + right % 16 >= 16,
            signed(left) + signed(right),
        )
    } else {
        (
            (left + modulus - right) % modulus,
            left < right,
            left % 16 < right % 16,
            signed(left) - signed(right),
        )
    };
    let even_parity = (0..8)
        .filter(|bit| result / 2_u64.pow(*bit) % 2 != 0)
        .count()
        % 2
        == 0;
    status_image([
        carry,
        even_parity,
        auxiliary,
        result == 0,
        result >= sign,
        mathematical < -(sign as i64) || mathematical >= sign as i64,
    ])
}

fn initial_cpu(kind: u8, left: u32, right: u32, expected: u8) -> CpuState {
    let mut cpu = CpuState::filled(0xa5);
    cpu.flags.status_source.kind = kind;
    cpu.flags.status_source.left = left;
    cpu.flags.status_source.right = right;
    cpu.flags.bytes = FlagBytes {
        cf: 0xfe,
        pf: 0x7f,
        af: 0x5a,
        zf: 0x80,
        sf: 0xff,
        of: 1,
        ..cpu.flags.bytes
    };
    // The host supplies the independently derived image for the test consumer.
    cpu.registers.eax = u32::from(expected);
    cpu
}

fn record_cases() -> Vec<CpuState> {
    let mut cases = Vec::new();
    for bits in 0_u8..64 {
        let mut cpu = initial_cpu(0, 0x1234_5678, 0x8765_4321, bits);
        cpu.flags.bytes = FlagBytes {
            cf: if bits & 1 != 0 { 0xff } else { 0xfe },
            pf: if bits & 2 != 0 { 0x7f } else { 0x80 },
            af: if bits & 4 != 0 { 0x81 } else { 0x5a },
            zf: if bits & 8 != 0 { 1 } else { 0 },
            sf: if bits & 16 != 0 { 0x55 } else { 0xaa },
            of: if bits & 32 != 0 { 3 } else { 2 },
            ..cpu.flags.bytes
        };
        cases.push(cpu);
    }
    for (width, sub_kind, add_kind, logic_kind) in [(8, 1, 2, 3), (16, 5, 6, 7), (32, 9, 10, 11)] {
        let sign = 2_u64.pow(width - 1) as u32;
        let maximum = (2_u64.pow(width) - 1) as u32;
        // Lazy records use dword carriers even for narrow operands. Their upper
        // bits are deliberately dirty; the oracle receives only logical values.
        let left_carrier = 0xa5a5_5a00 & !maximum;
        let right_carrier = 0x5a5a_a500 & !maximum;
        for (left, right) in [
            (0, 0),
            (maximum, 1),
            (sign - 1, 1),
            (sign, 1),
            (0, 1),
            (15, 1),
            (1, maximum),
        ] {
            for (kind, add) in [(sub_kind, false), (add_kind, true)] {
                cases.push(initial_cpu(
                    kind,
                    left | left_carrier,
                    right | right_carrier,
                    arithmetic_flags(width, add, left, right),
                ));
            }
        }
        for (result, expected) in [
            (0, 0b001010),
            (1, 0),
            (3, 0b000010),
            (sign, if width == 8 { 0b010000 } else { 0b010010 }),
            (maximum, 0b010010),
        ] {
            cases.push(initial_cpu(
                logic_kind,
                result | left_carrier,
                0xdead_beef,
                expected,
            ));
        }
    }
    cases
}

fn all_masks_reader() -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let expected = cpu_load!(&mut body, cpu.memory(), registers.eax)?;
                let mut failures = Val::<I64>::from(0);
                for bits in 0_u8..64 {
                    let actual = cpu.read_flags(&mut body, mask(bits))?;
                    let mut different = Val::<I1>::from(false);
                    for (index, value) in actual.into_iter().enumerate() {
                        let requested = bits & (1 << index) != 0;
                        assert_eq!(value.is_some(), requested, "mask {bits}, slot {index}");
                        if let Some(value) = value {
                            let expected_bit = expected.and(1_u32 << index).ne(0);
                            different = different.or(value.ne(expected_bit));
                        }
                    }
                    failures =
                        failures.or(different.unsigned().extend::<I64>().shl(u32::from(bits)));
                }
                body.return_(failures)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

#[test]
fn every_mask_reads_every_record_kind_without_changing_cpu_bytes() {
    let cases = record_cases();
    let module = TestModule::new(&all_masks_reader());
    let input = Input {
        patches_before_calls: cases
            .iter()
            .map(|cpu| CallPatches::cpu(vec![(0, cpu.to_bytes().to_vec())]))
            .collect(),
        ..Input::new(&cases[0].to_bytes())
    };
    // One module and instance check every mask across all record cases. A return
    // bit identifies the failing mask; zero means every subset matched.
    let observation = module.observe(&input, cases.len());
    assert!(observation.guest_unchanged);
    assert!(observation.machine_unchanged);
    assert_eq!(observation.events.len(), cases.len());
    for (event, cpu) in observation.events.iter().zip(&cases) {
        let Event::Return { outcome, snapshot } = event else {
            panic!("a flag reader must return without dispatching");
        };
        assert_eq!(
            outcome,
            &Outcome::Returned(vec![Argument::I64(0)]),
            "kind {}, operands {:08x}/{:08x}, expected {:02x}",
            cpu.flags.status_source.kind,
            cpu.flags.status_source.left,
            cpu.flags.status_source.right,
            cpu.registers.eax,
        );
        assert_eq!(&snapshot.cpu, &cpu.to_bytes().to_vec());
        assert_eq!(snapshot.guest, None);
    }
}

#[test]
fn subset_readers_share_matching_masks_and_return_separate_flags() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    for bits in [0b011110_u8, 0b011110, 0b000001] {
        program
            .function(
                Signature {
                    parameters: vec![],
                    results: vec![Type::I1; bits.count_ones() as usize],
                },
                |mut body| {
                    let flags = cpu.read_flags(&mut body, mask(bits))?;
                    body.return_(flags.into_iter().flatten().collect::<Vec<_>>())
                },
            )
            .unwrap();
    }
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut bodies = 0;
    let mut calls = Vec::new();
    let mut types = Vec::new();
    let mut functions = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                types.extend(section.into_iter_err_on_gc_types().map(Result::unwrap));
            }
            Payload::FunctionSection(section) => {
                functions.extend(section.into_iter().map(Result::unwrap));
            }
            Payload::CodeSectionEntry(body) => {
                bodies += 1;
                for operation in body.get_operators_reader().unwrap() {
                    if let Operator::Call { function_index } = operation.unwrap() {
                        calls.push(function_index);
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(bodies, 5, "three consumers share two subset readers");
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], calls[1]);
    assert_ne!(calls[1], calls[2]);
    for (target, count) in calls.into_iter().zip([4, 4, 1]) {
        let signature = &types[functions[target as usize] as usize];
        assert!(signature.params().is_empty());
        assert_eq!(
            signature.results(),
            vec![ValType::I32; count],
            "each requested logical flag must be a separate Wasm result"
        );
    }
}

#[test]
fn a_carry_only_consumer_needs_no_flag_unpacking() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I1],
            },
            |mut body| {
                let flags = cpu.read_flags(&mut body, FlagMask::of(StatusFlag::CF))?;
                body.return_(flags[StatusFlag::CF as usize].as_ref().unwrap())
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let operations = Parser::new(0)
        .parse_all(&bytes)
        .find_map(|payload| {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                Some(
                    body.get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap)
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        operations
            .iter()
            .filter(|operation| matches!(operation, Operator::Call { .. }))
            .count(),
        1
    );
    assert!(
        operations.iter().all(|operation| matches!(
            operation,
            Operator::Call { .. }
                | Operator::LocalGet { .. }
                | Operator::LocalSet { .. }
                | Operator::LocalTee { .. }
                | Operator::Return
                | Operator::End
        )),
        "the consumer must use the returned flag without transport arithmetic"
    );
}

#[test]
fn an_empty_mask_needs_no_reader_or_cpu_memory() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                results: vec![],
            },
            |mut body| {
                let flags = cpu.read_flags(&mut body, FlagMask::EMPTY)?;
                assert!(flags.into_iter().all(|value| value.is_none()));
                body.return_(())
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut operations = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap() {
            Payload::ImportSection(_) => panic!("an empty mask must not retain the CPU import"),
            Payload::CodeSectionEntry(body) => {
                operations.extend(
                    body.get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap),
                );
            }
            _ => {}
        }
    }
    assert!(matches!(
        operations.as_slice(),
        [Operator::Return, Operator::End]
    ));
}
