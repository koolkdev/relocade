use wasm86_x86::StatusFlags;

use crate::support::arithmetic;
use crate::support::conditions;
use crate::support::step;
use arithmetic::image;
use conditions::check_conditions;
use step::TestModule;

struct ArithmeticCase {
    name: &'static str,
    code: &'static [u8],
    eax: u32,
    ebx: u32,
    result: u32,
    kind: u8,
    left: u32,
    right: u32,
    // Bit n is the literal result of condition code n (O through G).
    conditions: u16,
}

const ARITHMETIC: &[ArithmeticCase] = &[
    ArithmeticCase {
        name: "byte nibble carry without unsigned or signed overflow",
        code: &[0x00, 0xd8],
        eax: 0x4433_223f,
        ebx: 1,
        result: 0x4433_2240,
        kind: 2,
        left: 0x3f,
        right: 1,
        conditions: 0xaaaa,
    },
    ArithmeticCase {
        name: "byte carry and signed overflow",
        code: &[0x02, 0xc3],
        eax: 0x4433_2280,
        ebx: 0x80,
        result: 0x4433_2200,
        kind: 2,
        left: 0x80,
        right: 0x80,
        conditions: 0x5655,
    },
    ArithmeticCase {
        name: "word carry and signed overflow",
        code: &[0x66, 0x01, 0xd8],
        eax: 0x4433_8000,
        ebx: 0xdead_8000,
        result: 0x4433_0000,
        kind: 6,
        left: 0x8000,
        right: 0x8000,
        conditions: 0x5655,
    },
    ArithmeticCase {
        name: "dword carry and signed overflow",
        code: &[0x03, 0xc3],
        eax: 0x8000_0000,
        ebx: 0x8000_0000,
        result: 0,
        kind: 10,
        left: 0x8000_0000,
        right: 0x8000_0000,
        conditions: 0x5655,
    },
    ArithmeticCase {
        name: "parity uses the low byte",
        code: &[0x05, 1, 0, 0, 0],
        eax: 0x100,
        ebx: 0,
        result: 0x101,
        kind: 10,
        left: 0x100,
        right: 1,
        conditions: 0xaaaa,
    },
    ArithmeticCase {
        name: "negative byte immediate sign extends for word ADD",
        code: &[0x66, 0x83, 0xc0, 0xff],
        eax: 0x4433_0001,
        ebx: 0,
        result: 0x4433_0000,
        kind: 6,
        left: 1,
        right: 0xffff,
        conditions: 0x6656,
    },
    ArithmeticCase {
        name: "byte compare separates signed and unsigned order",
        code: &[0x38, 0xd8],
        eax: 0x4433_227e,
        ebx: 0xfe,
        result: 0x4433_227e,
        kind: 1,
        left: 0x7e,
        right: 0xfe,
        conditions: 0xa965,
    },
    ArithmeticCase {
        name: "word compare separates signed and unsigned order",
        code: &[0x66, 0x3b, 0xc3],
        eax: 0x4433_7ffe,
        ebx: 0xdead_fffe,
        result: 0x4433_7ffe,
        kind: 5,
        left: 0x7ffe,
        right: 0xfffe,
        conditions: 0xa565,
    },
    ArithmeticCase {
        name: "dword compare separates signed and unsigned order",
        code: &[0x39, 0xd8],
        eax: 0x7fff_fffe,
        ebx: 0xffff_fffe,
        result: 0x7fff_fffe,
        kind: 9,
        left: 0x7fff_fffe,
        right: 0xffff_fffe,
        conditions: 0xa565,
    },
    ArithmeticCase {
        name: "group compare sign extends negative immediate",
        code: &[0x83, 0xf8, 0x80],
        eax: 0xffff_ff80,
        ebx: 0,
        result: 0xffff_ff80,
        kind: 9,
        left: 0xffff_ff80,
        right: 0xffff_ff80,
        conditions: 0x665a,
    },
    ArithmeticCase {
        name: "full group ADD carries across the dword",
        code: &[0x81, 0xc0, 0x80, 0x81, 0x83, 0x66],
        eax: 0x997c_7e80,
        ebx: 0,
        result: 0,
        kind: 10,
        left: 0x997c_7e80,
        right: 0x6683_8180,
        conditions: 0x6656,
    },
    ArithmeticCase {
        name: "full group CMP leaves an equal dword unchanged",
        code: &[0x81, 0xf8, 0x80, 0x81, 0x83, 0x66],
        eax: 0x6683_8180,
        ebx: 0,
        result: 0x6683_8180,
        kind: 9,
        left: 0x6683_8180,
        right: 0x6683_8180,
        conditions: 0x665a,
    },
    ArithmeticCase {
        name: "byte group CMP preserves signed overflow",
        code: &[0x80, 0xf8, 1],
        eax: 0x4433_2280,
        ebx: 0,
        result: 0x4433_2280,
        kind: 1,
        left: 0x80,
        right: 1,
        conditions: 0x5aa9,
    },
    ArithmeticCase {
        name: "accumulator byte CMP reports borrow",
        code: &[0x3c, 0xff],
        eax: 0x4433_2200,
        ebx: 0,
        result: 0x4433_2200,
        kind: 1,
        left: 0,
        right: 0xff,
        conditions: 0xaa66,
    },
    ArithmeticCase {
        name: "reverse byte CMP reads the register field first",
        code: &[0x3a, 0xc3],
        eax: 0x4433_22ff,
        ebx: 0,
        result: 0x4433_22ff,
        kind: 1,
        left: 0xff,
        right: 0,
        conditions: 0x55aa,
    },
    ArithmeticCase {
        name: "byte SUB borrows and preserves upper EAX",
        code: &[0x28, 0xd8],
        eax: 0x4433_2200,
        ebx: 1,
        result: 0x4433_22ff,
        kind: 1,
        left: 0,
        right: 1,
        conditions: 0x5566,
    },
    ArithmeticCase {
        name: "reverse byte SUB reads old AL before replacing AH",
        code: &[0x2a, 0xe0],
        eax: 0x4433_8001,
        ebx: 0,
        result: 0x4433_7f01,
        kind: 1,
        left: 0x80,
        right: 1,
        conditions: 0x5aa9,
    },
    ArithmeticCase {
        name: "word SUB retains signed overflow and low-byte parity",
        code: &[0x66, 0x29, 0xd8],
        eax: 0x4433_8000,
        ebx: 0xdead_0001,
        result: 0x4433_7fff,
        kind: 5,
        left: 0x8000,
        right: 1,
        conditions: 0x56a9,
    },
    ArithmeticCase {
        name: "dword SUB separates signed and unsigned order",
        code: &[0x2b, 0xc3],
        eax: 0x7fff_fffe,
        ebx: 0xffff_fffe,
        result: 0x8000_0000,
        kind: 9,
        left: 0x7fff_fffe,
        right: 0xffff_fffe,
        conditions: 0xa565,
    },
    ArithmeticCase {
        name: "group SUB sign extends its byte immediate to a word",
        code: &[0x66, 0x83, 0xe8, 0xff],
        eax: 0x4433_0000,
        ebx: 0,
        result: 0x4433_0001,
        kind: 5,
        left: 0,
        right: 0xffff,
        conditions: 0xaa66,
    },
    ArithmeticCase {
        name: "group SUB sign extends its byte immediate to a dword",
        code: &[0x83, 0xe8, 0x80],
        eax: 0xffff_ff80,
        ebx: 0,
        result: 0,
        kind: 9,
        left: 0xffff_ff80,
        right: 0xffff_ff80,
        conditions: 0x665a,
    },
];

#[test]
fn arithmetic_conditions() {
    let step = TestModule::interpreter();
    for case in ARITHMETIC {
        let mut image = image(case.code);
        image.cpu.registers.eax = case.eax;
        image.cpu.registers.ebx = case.ebx;
        let mut expected = image.cpu;
        expected.flags.kind = case.kind;
        expected.flags.left = case.left;
        expected.flags.right = case.right;
        expected.registers.eax = case.result;
        check_conditions(
            step,
            case.name,
            case.code,
            &mut image,
            &expected,
            case.conditions,
        );
    }
}

#[test]
fn incoming_records() {
    let step = TestModule::interpreter();
    for (name, kind, left, right, conditions) in [
        ("stored byte ADD", 2, 0x80, 0x80, 0x5655),
        ("stored word ADD", 6, 0x8000, 0x8000, 0x5655),
        ("stored dword ADD", 10, 0x8000_0000, 0x8000_0000, 0x5655),
        ("stored byte SUB", 1, 0x7e, 0xfe, 0xa965),
        ("stored word SUB", 5, 0x7ffe, 0xfffe, 0xa565),
        ("stored dword SUB", 9, 0x7fff_fffe, 0xffff_fffe, 0xa565),
        ("stored byte logic", 3, 0x80, 0x1234_5678, 0x59aa),
        ("stored word logic", 7, 0x8000, 0x1234_5678, 0x55aa),
        ("stored dword logic", 11, 0x8000_0000, 0x1234_5678, 0x55aa),
        (
            "stored byte logic ignores upper result bits",
            3,
            0x100,
            0x1234_5678,
            0x665a,
        ),
        (
            "stored word logic ignores upper result bits",
            7,
            0x10000,
            0x1234_5678,
            0x665a,
        ),
    ] {
        let mut image = image(&[]);
        image.cpu.flags.kind = kind;
        image.cpu.flags.left = left;
        image.cpu.flags.right = right;
        let expected = image.cpu;
        check_conditions(step, name, &[], &mut image, &expected, conditions);
    }
    let mut concrete = image(&[]);
    concrete.cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 1,
        af: 0,
        zf: 1,
        sf: 0,
        of: 0,
    };
    let expected = concrete.cpu;
    check_conditions(
        step,
        "concrete equal flags ignore stale recipe operands",
        &[],
        &mut concrete,
        &expected,
        0x665a,
    );
}
