use wasm86_x86::{compile_interpreter_step, CompiledModule};
use wasmparser::Validator;

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
#[allow(dead_code)]
#[path = "support/machine.rs"]
mod machine;
use machine::{both, Exit, Image, Step};
#[path = "support/arithmetic.rs"]
mod arithmetic;
use arithmetic::{image, recipe};

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
];

fn check_condition_sequence(
    flags: &[&str],
    step: &ModuleFile,
    name: &str,
    prefix: &[u8],
    image: &mut Image,
    prefix_updates: &[(usize, u32)],
    conditions: u16,
) {
    let mut code = prefix.to_vec();
    for condition in 0..16 {
        // ModRM.reg is ignored by SETcc. Every possible value appears here.
        code.extend_from_slice(&[
            0x0f,
            0x90 + condition,
            0x47 | ((condition & 7) << 3),
            condition,
        ]);
    }
    image.data(0x3000, &code);
    image.register(52, 0x6000);
    image.map(6, 0xa000, true);
    image.data(0x9fff, &[0xa5; 18]);
    let results = (0..16)
        .map(|condition| [((conditions >> condition) & 1) as u8])
        .collect::<Vec<_>>();
    let mut updates = Vec::new();
    if !prefix.is_empty() {
        let mut changes = prefix_updates.to_vec();
        changes.extend_from_slice(&[(56, 0x1000 + prefix.len() as u32), (144, 0)]);
        updates.push(changes);
    }
    for condition in 0..16 {
        updates.push(vec![
            (56, 0x1000 + prefix.len() as u32 + 4 * (condition + 1)),
            (144, condition + u32::from(!prefix.is_empty())),
        ]);
    }
    let writes = results
        .iter()
        .enumerate()
        .map(|(condition, result)| [(0xa000 + condition as u32, result.as_slice())])
        .collect::<Vec<_>>();
    let mut steps = Vec::new();
    if !prefix.is_empty() {
        steps.push(Step {
            cpu: &updates[0],
            ram: &[],
            exit: Exit::Dispatch(0x1000 + prefix.len() as u32),
        });
    }
    for condition in 0..16 {
        steps.push(Step {
            cpu: &updates[condition + usize::from(!prefix.is_empty())],
            ram: &writes[condition],
            exit: Exit::Dispatch(0x1000 + prefix.len() as u32 + 4 * (condition as u32 + 1)),
        });
    }
    both(step, flags, name, &code, steps.len() as u32, image, &steps);
}

fn check_arithmetic_conditions(flags: &[&str], step: &ModuleFile) {
    for case in ARITHMETIC {
        let mut image = image(case.code);
        image.register(24, case.eax);
        image.register(36, case.ebx);
        let mut updates = recipe(case.kind, case.left, case.right).to_vec();
        updates.push((24, case.result));
        check_condition_sequence(
            flags,
            step,
            case.name,
            case.code,
            &mut image,
            &updates,
            case.conditions,
        );
    }
}

fn check_incoming_records(flags: &[&str], step: &ModuleFile) {
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
    ] {
        let mut image = image(&[]);
        for (offset, value) in recipe(kind, left, right) {
            image.register(offset, value);
        }
        check_condition_sequence(flags, step, name, &[], &mut image, &[], conditions);
    }
    let mut concrete = image(&[]);
    concrete.cpu[12..18].copy_from_slice(&[0, 1, 0, 1, 0, 0]);
    check_condition_sequence(
        flags,
        step,
        "concrete equal flags ignore stale recipe operands",
        &[],
        &mut concrete,
        &[],
        0x665a,
    );
}

fn execute(flags: &[&str]) {
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let step = ModuleFile::new(&module);
    check_arithmetic_conditions(flags, &step);
    check_incoming_records(flags, &step);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn arithmetic_and_conditions_execute_in_v8() {
    execute(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn arithmetic_and_conditions_execute_in_optimizing_v8() {
    execute(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
