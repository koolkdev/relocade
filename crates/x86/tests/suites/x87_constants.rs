//! Architectural constants retain extended precision and have distinct load rounding.

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Step},
    step::{Engine, TestModule},
    x87::{complete_x87, dispatch, set_control, stack_image, write_value, INDEFINITE},
};
use wasm86_x86::{Compiler, SegmentProfile};
use wasmparser::{Operator, Parser, Payload};

// Literal extended encodings for nearest, down, up and truncate. The five
// irrational constants are rounded by RC, independently of precision control.
const CONSTANTS: &[(u8, [u64; 4], u16)] = &[
    (0xe8, [0x8000_0000_0000_0000; 4], 0x3fff),
    (
        0xe9,
        [
            0xd49a_784b_cd1b_8afe,
            0xd49a_784b_cd1b_8afe,
            0xd49a_784b_cd1b_8aff,
            0xd49a_784b_cd1b_8afe,
        ],
        0x4000,
    ),
    (
        0xea,
        [
            0xb8aa_3b29_5c17_f0bc,
            0xb8aa_3b29_5c17_f0bb,
            0xb8aa_3b29_5c17_f0bc,
            0xb8aa_3b29_5c17_f0bb,
        ],
        0x3fff,
    ),
    (
        0xeb,
        [
            0xc90f_daa2_2168_c235,
            0xc90f_daa2_2168_c234,
            0xc90f_daa2_2168_c235,
            0xc90f_daa2_2168_c234,
        ],
        0x4000,
    ),
    (
        0xec,
        [
            0x9a20_9a84_fbcf_f799,
            0x9a20_9a84_fbcf_f798,
            0x9a20_9a84_fbcf_f799,
            0x9a20_9a84_fbcf_f798,
        ],
        0x3ffd,
    ),
    (
        0xed,
        [
            0xb172_17f7_d1cf_79ac,
            0xb172_17f7_d1cf_79ab,
            0xb172_17f7_d1cf_79ac,
            0xb172_17f7_d1cf_79ab,
        ],
        0x3ffe,
    ),
    (0xee, [0; 4], 0),
];

fn rounded_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for &(opcode, significands, exponent) in CONSTANTS {
        let code = [0xd9, opcode, 0x9b];
        for pc in [0, 2, 3] {
            for (rc, significand) in significands.into_iter().enumerate() {
                let mut image = stack_image(&code, 0, 0xffff);
                image.cpu.x87.status.precision = 0;
                // Unmask precision: constant rounding must not create a pending exception.
                set_control(
                    &mut image.cpu.x87.control,
                    0x005f | (pc << 8) | ((rc as u16) << 10),
                );
                let mut loaded = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
                loaded.x87.status.top = 7;
                loaded.x87.status.c1 = 0;
                write_value(&mut loaded, 7, (significand, exponent));
                let mut waited = loaded;
                waited.eip += 1;
                waited.instruction_count += 1;
                checks.check(
                    &format!("constant {opcode:x}, PC={pc}, RC={rc}"),
                    &code,
                    &image,
                    &[dispatch(loaded), dispatch(waited)],
                );
            }
        }
    }
}

fn stack_and_pending_exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0xeb, 0xee] {
        let code = [0xd9, opcode, 0x9b];
        for masked in [false, true] {
            let mut image = stack_image(&code, 3, 0);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037e },
            );
            let mut cpu = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
            cpu.x87.status.invalid = 1;
            cpu.x87.status.stack_fault = 1;
            cpu.x87.status.c1 = 1;
            if masked {
                cpu.x87.status.top = 2;
                write_value(&mut cpu, 2, INDEFINITE);
            } else {
                cpu.x87.status.error_summary = 1;
                cpu.x87.status.busy = 1;
            }
            let mut waited = cpu;
            if masked {
                waited.eip += 1;
                waited.instruction_count += 1;
            }
            checks.check(
                "constant push on a full stack",
                &code,
                &image,
                &[
                    dispatch(cpu),
                    Step {
                        cpu: waited,
                        ram: &[],
                        exit: if masked {
                            Exit::Dispatch(waited.eip)
                        } else {
                            Exit::FloatingPoint
                        },
                    },
                ],
            );
        }
    }
    let code = [0xd9, 0xe8];
    let mut image = stack_image(&code, 0, 0xffff);
    set_control(&mut image.cpu.x87.control, 0x037e);
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception prevents a constant push",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

fn changed_rounding_control(engine: Engine, frontend: Frontend) {
    let code = [
        0xd9, 0x2d, 0, 0x40, 0, 0, 0xd9, 0xeb, 0xd9, 0x2d, 2, 0x40, 0, 0, 0xd9, 0xeb,
    ];
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0x7f, 0x04, 0x7f, 0x08]);
    let mut control_down = image.cpu;
    control_down.eip += 6;
    control_down.instruction_count = control_down.instruction_count.wrapping_add(1);
    set_control(&mut control_down.x87.control, 0x047f);
    let mut down = complete_x87(control_down, 2, 0x01eb);
    down.x87.status.top = 7;
    down.x87.status.c1 = 0;
    write_value(&mut down, 7, (0xc90f_daa2_2168_c234, 0x4000));
    let mut control_up = down;
    control_up.eip += 6;
    control_up.instruction_count = control_up.instruction_count.wrapping_add(1);
    set_control(&mut control_up.x87.control, 0x087f);
    let mut up = complete_x87(control_up, 2, 0x01eb);
    up.x87.status.top = 6;
    write_value(&mut up, 6, (0xc90f_daa2_2168_c235, 0x4000));
    ImageSequences::new(engine, frontend, SegmentProfile::Flat32).check(
        "each constant load consumes the current rounding control",
        &code,
        &image,
        &[
            dispatch(control_down),
            dispatch(down),
            dispatch(control_up),
            dispatch(up),
        ],
    );
}

fn exact_constants_keep_native_arithmetic(engine: Engine) {
    // 1 -> -1 -> -1 + 3 -> 0 + 2. Both exact constants keep native significands.
    let code = [
        0xd9, 0xe8, 0xd9, 0xe0, 0xda, 0x05, 0, 0x40, 0, 0, 0xd9, 0xee, 0xde, 0xc1,
    ];
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    set_control(&mut image.cpu.x87.control, 0x027f);
    image.map(4, 0x8000, false);
    image.data(0x8000, &3_i32.to_le_bytes());
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&image.cpu)
        .compile_block(0x1000, &code, 5)
        .unwrap();
    assert!(Parser::new(0).parse_all(&compiled.bytes).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if body.get_operators_reader().unwrap()
            .into_iter().any(|op| matches!(op.unwrap(), Operator::F64Add)))
    }));
    let mut cpu = complete_x87(image.cpu, 2, 0x01e8);
    cpu = complete_x87(cpu, 2, 0x01e0);
    cpu = complete_x87(cpu, 6, 0x0205);
    cpu = complete_x87(cpu, 2, 0x01ee);
    cpu = complete_x87(cpu, 2, 0x06c1);
    cpu.x87.status.top = 7;
    cpu.x87.status.c1 = 0;
    cpu.x87.data_offset = 0x4000;
    cpu.x87.data_selector = 0x23;
    write_value(&mut cpu, 6, (0, 0));
    cpu.x87.tag_word |= 3 << 12;
    write_value(&mut cpu, 7, (0x8000_0000_0000_0000, 0x4000));
    assert_eq!(
        engine.observe(&TestModule::new(&compiled), &image.input(), 1),
        expected(&image, &[dispatch(cpu)])
    );
}

#[test]
fn encodings() {
    for &(opcode, _, _) in CONSTANTS {
        crate::support::encoding::check_length(&[0xd9, opcode]);
    }
}

test_frontends!(values, rounded_values);
test_frontends!(faults, stack_and_pending_exceptions);
test_frontends!(control_changes, changed_rounding_control);
#[test]
fn native_constant_sequence() {
    exact_constants_keep_native_arithmetic(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_constant_sequence() {
    exact_constants_keep_native_arithmetic(Engine::V8);
}
