use wasm86_x86::compile_block_from_bytes;
use wasmparser::Validator;

use crate::support::arithmetic;
use crate::support::conditions;
use crate::support::machine;
use crate::support::step;
use arithmetic::image;
use conditions::check_conditions;
use machine::{both, check, Exit, Step};
use step::TestModule;

struct LogicalCase {
    name: &'static str,
    code: &'static [u8],
    eax: u32,
    ebx: u32,
    final_eax: u32,
    kind: u8,
    result: u32,
    // Bit n is the literal result of condition code n (O through G).
    conditions: u16,
}

const LOGICAL: &[LogicalCase] = &[
    LogicalCase {
        name: "byte AND retains upper EAX and clears stale carry and overflow",
        code: &[0x20, 0xd8],
        eax: 0x4433_22f3,
        ebx: 0x0f,
        final_eax: 0x4433_2203,
        kind: 3,
        result: 3,
        conditions: 0xa6aa,
    },
    LogicalCase {
        name: "word AND uses the word sign and only the low byte for parity",
        code: &[0x66, 0x21, 0xd8],
        eax: 0x4433_80ff,
        ebx: 0xdead_ff00,
        final_eax: 0x4433_8000,
        kind: 7,
        result: 0x8000,
        conditions: 0x55aa,
    },
    LogicalCase {
        name: "reverse dword AND records a negative odd-parity result",
        code: &[0x23, 0xc3],
        eax: 0x8000_0001,
        ebx: 0xffff_ffff,
        final_eax: 0x8000_0001,
        kind: 11,
        result: 0x8000_0001,
        conditions: 0x59aa,
    },
    LogicalCase {
        name: "group AND sign extends its byte mask to a dword",
        code: &[0x83, 0xe0, 0xff],
        eax: 0x89ab_cdef,
        ebx: 0,
        final_eax: 0x89ab_cdef,
        kind: 11,
        result: 0x89ab_cdef,
        conditions: 0x59aa,
    },
    LogicalCase {
        name: "byte OR reads old AL before replacing AH",
        code: &[0x0a, 0xe0],
        eax: 0x4433_8001,
        ebx: 0,
        final_eax: 0x4433_8101,
        kind: 3,
        result: 0x81,
        conditions: 0x55aa,
    },
    LogicalCase {
        name: "group OR sign extends its byte mask to a word",
        code: &[0x66, 0x83, 0xc8, 0x80],
        eax: 0x4433_0001,
        ebx: 0,
        final_eax: 0x4433_ff81,
        kind: 7,
        result: 0xff81,
        conditions: 0x55aa,
    },
    LogicalCase {
        name: "dword accumulator OR ignores upper bits for parity",
        code: &[0x0d, 1, 0, 0, 0],
        eax: 0x100,
        ebx: 0,
        final_eax: 0x101,
        kind: 11,
        result: 0x101,
        conditions: 0xaaaa,
    },
    LogicalCase {
        name: "self XOR clears only AH and produces equal flags",
        code: &[0x30, 0xe4],
        eax: 0x4433_ff80,
        ebx: 0,
        final_eax: 0x4433_0080,
        kind: 3,
        result: 0,
        conditions: 0x665a,
    },
    LogicalCase {
        name: "reverse word XOR preserves the upper parent register",
        code: &[0x66, 0x33, 0xc3],
        eax: 0x4433_ffff,
        ebx: 0xdead_8000,
        final_eax: 0x4433_7fff,
        kind: 7,
        result: 0x7fff,
        conditions: 0xa6aa,
    },
    LogicalCase {
        name: "group XOR sign extends its byte mask to a dword",
        code: &[0x83, 0xf0, 0xff],
        eax: 0x8000_0000,
        ebx: 0,
        final_eax: 0x7fff_ffff,
        kind: 11,
        result: 0x7fff_ffff,
        conditions: 0xa6aa,
    },
    LogicalCase {
        name: "TEST reads AH and AL without replacing either alias",
        code: &[0x84, 0xc4],
        eax: 0x4433_807f,
        ebx: 0,
        final_eax: 0x4433_807f,
        kind: 3,
        result: 0,
        conditions: 0x665a,
    },
    LogicalCase {
        name: "word accumulator TEST leaves EAX unchanged",
        code: &[0x66, 0xa9, 0x00, 0xff],
        eax: 0x4433_8001,
        ebx: 0,
        final_eax: 0x4433_8001,
        kind: 7,
        result: 0x8000,
        conditions: 0x55aa,
    },
    LogicalCase {
        name: "dword register TEST keeps both inputs",
        code: &[0x85, 0xd8],
        eax: 0x8000_0001,
        ebx: 0xffff_ffff,
        final_eax: 0x8000_0001,
        kind: 11,
        result: 0x8000_0001,
        conditions: 0x59aa,
    },
    LogicalCase {
        name: "byte group TEST ignores the operand-size prefix",
        code: &[0x66, 0xf6, 0xc0, 0x80],
        eax: 0x4433_2280,
        ebx: 0,
        final_eax: 0x4433_2280,
        kind: 3,
        result: 0x80,
        conditions: 0x59aa,
    },
    LogicalCase {
        name: "dword group TEST publishes its result without a write",
        code: &[0xf7, 0xc0, 0x0f, 0, 0, 0],
        eax: 0xffff_fff0,
        ebx: 0,
        final_eax: 0xffff_fff0,
        kind: 11,
        result: 0,
        conditions: 0x665a,
    },
];

#[test]
fn replacing_arithmetic() {
    let step = TestModule::interpreter();
    let code = [
        0x05, 1, 0, 0, 0, // ADD EAX,1
        0x31, 0xc0, // XOR EAX,EAX
        0x0f, 0x94, 0xc4, // SETE AH
        0xb0, 0x7f, // MOV AL,7f
        0x0f, 0x92, 0xc0, // SETB AL
    ];
    let mut image = image(&code);
    image.cpu.registers.eax = 0xffff_ffff;
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 10;
    expected_cpu.flags.left = 0xffff_ffff;
    expected_cpu.flags.right = 1;
    expected_cpu.registers.eax = 0;
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    });

    expected_cpu.flags.kind = 11;
    expected_cpu.flags.left = 0;
    expected_cpu.eip = 0x1007;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1007),
    });

    expected_cpu.registers.eax = 0x100;
    expected_cpu.eip = 0x100a;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100a),
    });

    expected_cpu.registers.eax = 0x17f;
    expected_cpu.eip = 0x100c;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100c),
    });

    expected_cpu.registers.eax = 0x100;
    expected_cpu.eip = 0x100f;
    expected_cpu.instruction_count = 4;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100f),
    });

    check(
        step,
        "published arithmetic B remains unused after logic",
        &image,
        &steps,
    );

    let snapshot = compile_block_from_bytes(0x1000, &code, 5).unwrap();
    Validator::new().validate_all(&snapshot.bytes).unwrap();
    // One snapshot never publishes the replaced ADD record: its unused B stays
    // at the original backing value. Both paths expose the same logical flags.
    expected_cpu.flags.right = image.cpu.flags.right;

    check(
        &TestModule::new(&snapshot),
        "logic discards an unpublished arithmetic B",
        &image,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x100f),
        }],
    );
}

#[test]
fn replacing_logic() {
    let step = TestModule::interpreter();
    let code = [
        0x66, 0x25, 0xff, 0, // AND AX,00ff
        0x2d, 1, 0, 0x33, 0x44, // SUB EAX,44330001
        0x0f, 0x94, 0xc4, // SETE AH
    ];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4433_8001;
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 7;
    expected_cpu.flags.left = 1;
    expected_cpu.registers.eax = 0x4433_0001;
    expected_cpu.eip = 0x1004;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1004),
    });

    expected_cpu.flags.kind = 9;
    expected_cpu.flags.left = 0x4433_0001;
    expected_cpu.flags.right = 0x4433_0001;
    expected_cpu.registers.eax = 0;
    expected_cpu.eip = 0x1009;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1009),
    });

    expected_cpu.registers.eax = 0x100;
    expected_cpu.eip = 0x100c;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100c),
    });

    both(
        step,
        "arithmetic replaces a logical result with its original operands",
        &code,
        3,
        &image,
        &steps,
    );
}

#[test]
fn logical_results_and_conditions() {
    let step = TestModule::interpreter();
    for case in LOGICAL {
        let mut image = image(case.code);
        image.cpu.registers.eax = case.eax;
        image.cpu.registers.ebx = case.ebx;
        // Logic publishes kind and result. The unused B field remains untouched.
        let mut expected = image.cpu;
        expected.flags.kind = case.kind;
        expected.flags.left = case.result;
        expected.registers.eax = case.final_eax;
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
