//! Exact comparisons publish condition codes without modifying operand payloads.

#[path = "x87_compare/exceptions.rs"]
mod exceptions;
#[path = "x87_compare/integer.rs"]
mod integer;
#[path = "x87_compare/jit.rs"]
mod jit;
#[path = "x87_compare/memory.rs"]
mod memory;
#[path = "x87_compare/sequences.rs"]
mod sequences;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{complete_x87, dispatch, set_control, stack_image, status, write_value},
};
use wasm86_x86::{CpuState, SegmentProfile};

const LEADING: u64 = 1 << 63;
const ONE: (u64, u16) = (LEADING, 0x3fff);
const QNAN: (u64, u16) = (0xc000_0000_0000_0042, 0x7fff);
const SNAN: (u64, u16) = (LEADING + 1, 0xffff);
const LESS: u16 = 0x0100;
const EQUAL: u16 = 0x4000;
const UNORDERED: u16 = 0x4500;
const PENDING: u16 = 0x8080;

fn initial_image(code: &[u8]) -> Image {
    let mut image = stack_image(code, 7, 0);
    // Mixed prior condition codes distinguish suppressed results from unordered.
    // PE is already sticky and must survive every comparison.
    image.cpu.x87.status = status(0x7b20);
    image
}

fn completed(mut cpu: CpuState, length: u32, opcode: u16, flags: u16, pops: u8) -> CpuState {
    cpu = complete_x87(cpu, length, opcode);
    let top = cpu.x87.status.top;
    cpu.x87.status = status((u16::from(top) << 11) | 0x20 | flags);
    for index in 0..pops {
        cpu.x87.tag_word |= 3 << (((top + index) & 7) * 2);
    }
    cpu.x87.status.top = (top + pops) & 7;
    cpu
}

#[derive(Debug)]
struct Case {
    left: (u64, u16),
    right: (u64, u16),
    control: u16,
    // Literal comparison and exception bits; TOP and the prior sticky PE are
    // supplied by the fixture. `pops` is the expected committed movement.
    flags: u16,
    pops: u8,
    empty: u8,
}

fn check_case(checks: &mut ImageSequences, name: &str, instruction: [u8; 2], case: Case) {
    let code = [instruction.as_slice(), &[0xdf, 0xe0, 0x9b]].concat();
    let mut image = initial_image(&code);
    set_control(&mut image.cpu.x87.control, case.control);
    write_value(&mut image.cpu, 7, case.left);
    write_value(&mut image.cpu, 0, case.right);
    if case.empty & 1 != 0 {
        image.cpu.x87.tag_word |= 0xc000;
    }
    if case.empty & 2 != 0 {
        image.cpu.x87.tag_word |= 3;
    }
    let opcode = (u16::from(instruction[0] & 7) << 8) | u16::from(instruction[1]);
    let result = completed(image.cpu, 2, opcode, case.flags, case.pops);
    let mut observed = result;
    observed.eip += 2;
    observed.instruction_count += 1;
    observed.registers.eax =
        0x1111_0000 | u32::from((u16::from(result.x87.status.top) << 11) | 0x20 | case.flags);
    let mut waited = observed;
    let exit = if case.flags & PENDING != 0 {
        Exit::FloatingPoint
    } else {
        waited.eip += 1;
        waited.instruction_count += 1;
        Exit::Dispatch(waited.eip)
    };
    checks.check(
        &format!("{name}: {instruction:02x?} {case:x?}"),
        &code,
        &image,
        &[
            dispatch(result),
            dispatch(observed),
            Step {
                cpu: waited,
                ram: &[],
                exit,
            },
        ],
    );
}

fn register_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (code, source, pops) in [
        ([0xd8, 0xd2], 1, 0), // FCOM ST2
        ([0xd8, 0xda], 1, 1), // FCOMP ST2
        ([0xde, 0xd9], 0, 2), // FCOMPP
        ([0xdd, 0xe2], 1, 0), // FUCOM ST2
        ([0xdd, 0xea], 1, 1), // FUCOMP ST2
        ([0xda, 0xe9], 0, 2), // FUCOMPP
    ] {
        let mut image = initial_image(&code);
        write_value(&mut image.cpu, 7, ONE);
        write_value(&mut image.cpu, source, (LEADING, 0x4000));
        let opcode = (u16::from(code[0] & 7) << 8) | u16::from(code[1]);
        let result = completed(image.cpu, 2, opcode, LESS, pops);
        checks.check(
            "register source and wrapping pop",
            &code,
            &image,
            &[dispatch(result)],
        );
    }
    for (code, pops) in [
        ([0xd8, 0xd0], 0),
        ([0xd8, 0xd8], 1),
        ([0xdd, 0xe0], 0),
        ([0xdd, 0xe8], 1),
    ] {
        let mut image = initial_image(&code);
        write_value(&mut image.cpu, 7, (LEADING + 1, 0xbfff));
        let opcode = (u16::from(code[0] & 7) << 8) | u16::from(code[1]);
        let result = completed(image.cpu, 2, opcode, EQUAL, pops);
        checks.check(
            "self comparison reads the entry value",
            &code,
            &image,
            &[dispatch(result)],
        );
    }
}

fn exact_ordering(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, right, flags) in [
        (ONE, ONE, EQUAL),
        ((LEADING + 1, 0x3fff), ONE, 0),
        (ONE, (LEADING + 1, 0x3fff), LESS),
        ((LEADING + 1, 0xbfff), (LEADING, 0xbfff), LESS),
        ((LEADING, 0xbfff), (LEADING + 1, 0xbfff), 0),
        ((u64::MAX, 0x3ffe), ONE, LESS),
        ((u64::MAX, 0xbffe), (LEADING, 0xbfff), 0),
        ((LEADING, 0xbfff), ONE, LESS),
        (ONE, (LEADING, 0xbfff), 0),
        ((0, 0), (0, 0x8000), EQUAL),
        ((0, 0x8000), (0, 0), EQUAL),
        ((0, 0x8000), ONE, LESS),
        ((0, 0), (LEADING, 0xbfff), 0),
        ((LEADING, 0x7fff), (LEADING, 0x7fff), EQUAL),
        ((LEADING, 0xffff), (LEADING, 0x7fff), LESS),
        ((u64::MAX, 0x7ffe), (LEADING, 0x7fff), LESS),
        ((LEADING, 0xffff), (u64::MAX, 0xfffe), LESS),
        ((LEADING, 1), (LEADING, 0x7ffe), LESS),
    ] {
        check_case(
            &mut checks,
            "full extended ordering",
            [0xd8, 0xd1],
            Case {
                left,
                right,
                control: 0x037f,
                flags,
                pops: 0,
                empty: 0,
            },
        );
    }
    // The low significand bit still decides the comparison under every PC/RC.
    for pc in [0, 2, 3] {
        for rc in 0..4 {
            check_case(
                &mut checks,
                "comparison ignores precision and rounding controls",
                [0xdd, 0xe9],
                Case {
                    left: (LEADING + 1, 0x3fff),
                    right: ONE,
                    control: 0x007f | (pc << 8) | (rc << 10),
                    flags: 0,
                    pops: 1,
                    empty: 0,
                },
            );
        }
    }
}

fn test_against_zero(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, flags) in [
        (ONE, 0),
        ((LEADING, 0xbfff), LESS),
        ((0, 0), EQUAL),
        ((0, 0x8000), EQUAL),
        ((LEADING, 0x7fff), 0),
        ((LEADING, 0xffff), LESS),
        (QNAN, UNORDERED | 1),
        (SNAN, UNORDERED | 1),
        ((1, 0), 2),
        ((1, 0x8000), LESS | 2),
    ] {
        check_case(
            &mut checks,
            "FTST has a constant positive-zero source",
            [0xd9, 0xe4],
            Case {
                left,
                right: SNAN,
                control: 0x037f,
                flags,
                pops: 0,
                empty: 2,
            },
        );
    }
}

test_frontends!(forms, register_forms);
test_frontends!(ordering, exact_ordering);
test_frontends!(zero, test_against_zero);

#[test]
fn complete_encodings() {
    use crate::support::encoding::check_length;
    for code in [
        &[0xd8, 0xd3][..],
        &[0xd8, 0xdb],
        &[0xde, 0xd9],
        &[0xdd, 0xe3],
        &[0xdd, 0xeb],
        &[0xda, 0xe9],
        &[0xd9, 0xe4],
        &[0xd8, 0x15, 0, 0x40, 0, 0],
        &[0xdc, 0x15, 0, 0x40, 0, 0],
        &[0xd8, 0x1d, 0, 0x40, 0, 0],
        &[0xdc, 0x1d, 0, 0x40, 0, 0],
    ] {
        check_length(code);
    }
}
