//! Relative branch forms, target arithmetic and condition consumers.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set, Undefined},
        Flags, InstructionCase as Case,
    },
    conditions::CONDITION_EXAMPLES,
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes,
    Gpr32::{self, Eax, Ebx},
};

struct ConditionalForm {
    code: &'static [u8],
    condition_byte: usize,
    taken: u32,
    fallthrough: u32,
}

#[rustfmt::skip]
const CONDITIONAL_FORMS: [ConditionalForm; 3] = [
    ConditionalForm { code: &[0x70, 0x7f], condition_byte: 0, taken: 0x1081, fallthrough: 0x1002 },
    ConditionalForm { code: &[0x0f, 0x80, 0x7f, 0, 0, 0], condition_byte: 1, taken: 0x1085, fallthrough: 0x1006 },
    ConditionalForm { code: &[0x66, 0x0f, 0x80, 0x7f, 0], condition_byte: 2, taken: 0x1084, fallthrough: 0x1005 },
];

fn condition_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    // Every condition appears in all three forms; inputs sample forms instead of multiplying them.
    for (form, example) in CONDITIONAL_FORMS.iter().cycle().zip(CONDITION_EXAMPLES) {
        for (condition, taken) in example.results.into_iter().enumerate() {
            let mut code = form.code.to_vec();
            code[form.condition_byte] += condition as u8;
            cases.push(
                Case::new(
                    format!("{}, Jcc {code:02x?}", example.name),
                    &code,
                    example.flags,
                    Flags::all(Preserved),
                )
                .preserve_flag_record()
                .dispatch(if taken { form.taken } else { form.fallthrough }),
            );
        }
    }
    cases
}

test_cases!(conditions_across_short_and_near_forms, condition_cases());

#[rustfmt::skip]
fn pending_conditions() -> Vec<Sequence> {
    struct Producer {
        name: &'static str,
        code: &'static [u8],
        inputs: &'static [(Gpr32, u32)],
        outputs: &'static [(Gpr32, u32)],
        flags: Flags<FlagExpectation>,
        outcomes: [(u8, bool); 4],
    }
    let producers = [
        Producer { name: "local ADD", code: &[0x01, 0xc0], inputs: &[(Eax, 0x8000_0000)], outputs: &[(Eax, 0)],
            flags: Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set },
            outcomes: [(0, true), (3, false), (4, true), (8, false)] },
        Producer { name: "local CMP", code: &[0x39, 0xd8], inputs: &[(Eax, 0x7fff_fffe), (Ebx, 0xffff_fffe)], outputs: &[],
            flags: Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set },
            outcomes: [(2, true), (12, false), (13, true), (15, true)] },
        Producer { name: "local TEST odd result", code: &[0x85, 0xc0], inputs: &[(Eax, 1)], outputs: &[],
            flags: Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Clear, of: Clear },
            outcomes: [(4, false), (5, true), (10, false), (11, true)] },
        Producer { name: "local TEST zero result", code: &[0x85, 0xc0], inputs: &[(Eax, 0)], outputs: &[],
            flags: Flags { cf: Clear, pf: Set, af: Undefined, zf: Set, sf: Clear, of: Clear },
            outcomes: [(4, true), (5, false), (10, true), (11, false)] },
        Producer { name: "local ADC", code: &[0x83, 0xd0, 0], inputs: &[(Eax, 0x7fff_ffff)], outputs: &[(Eax, 0x8000_0000)],
            flags: Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set },
            outcomes: [(0, true), (2, false), (12, false), (15, true)] },
    ];
    let mut cases = Vec::new();
    for producer in producers {
        for (form, (condition, taken)) in CONDITIONAL_FORMS.iter().cycle().zip(producer.outcomes) {
            let mut branch = form.code.to_vec();
            branch[form.condition_byte] += condition;
            let mut first = Step::new(producer.code, producer.flags);
            for &(register, value) in producer.outputs { first = first.register(register, value); }
            let target = (if taken { form.taken } else { form.fallthrough }) + producer.code.len() as u32;
            cases.push(Sequence::new(format!("{} then {branch:02x?}", producer.name), Flags::all(true))
                .initial_registers(producer.inputs).step(first)
                .step(Step::preserving_flags(&branch).dispatch(target)));
        }
    }
    cases
}
test_sequences!(pending_arithmetic_conditions, pending_conditions());

#[rustfmt::skip]
fn jump_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (start, code, target) in [
        (0x1000, &[0xeb, 0][..], 0x1002),
        (0x1000, &[0xeb, 0x7f], 0x1081),
        (0x1000, &[0xeb, 0x80], 0x0f82),
        (0x1000, &[0xeb, 0xfe], 0x1000),
        (0, &[0xeb, 0x80], 0xffff_ff82),
        (0xffff_fffe, &[0xeb, 0x7f], 0x7f),
        (0x1000, &[0xe9, 0, 0, 0, 0], 0x1005),
        (0x1000, &[0xe9, 0xff, 0xff, 0xff, 0x7f], 0x8000_1004),
        (0x1000, &[0xe9, 0, 0, 0, 0x80], 0x8000_1005),
        (0, &[0xe9, 0xfa, 0xff, 0xff, 0xff], 0xffff_ffff),
        (0xffff_fffc, &[0xe9, 0, 0, 0, 0], 1),
        (0x1234_fffe, &[0x66, 0xeb, 0], 1),
        (0x1234_1000, &[0x66, 0xeb, 0x80], 0x0f83),
        (0x1234_1000, &[0x66, 0xe9, 0, 0], 0x1004),
        (0x1234_1000, &[0x66, 0xe9, 0xff, 0x7f], 0x9003),
        (0x1234_1000, &[0x66, 0xe9, 0, 0x80], 0x9004),
        (0x1234_fffe, &[0x66, 0xe9, 0xfd, 0xff], 0xffff),
        (0x1234_1000, &[0x66, 0x66, 0xe9, 0xfb, 0xff], 0x1000),
    ] {
        cases.push(Case::preserving_flags(format!("JMP {code:02x?} at {start:08x}"), code)
            .at(start).dispatch(target));
    }
    cases
}

#[rustfmt::skip]
fn word_condition_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (start, code, taken_target, fallthrough) in [
        (0x1234_fffe, &[0x66, 0x74, 0][..], 1, 0x1235_0001),
        (0x1234_1000, &[0x66, 0x74, 0x80], 0x0f83, 0x1234_1003),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0, 0][..],
            0x1005,
            0x1234_1005,
        ),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0xff, 0x7f],
            0x9004,
            0x1234_1005,
        ),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0, 0x80],
            0x9005,
            0x1234_1005,
        ),
        (
            0x1234_fffe,
            &[0x66, 0x0f, 0x84, 0xfc, 0xff],
            0xffff,
            0x1235_0003,
        ),
        (0xffff_fffd, &[0x66, 0x0f, 0x84, 0xfd, 0xff], 0xffff, 2),
    ] {
        for (zero, target) in [(false, fallthrough), (true, taken_target)] {
            let flags = Flags { cf: true, pf: true, af: true, zf: zero, sf: true, of: true };
            cases.push(Case::new(format!("word JE {code:02x?} at {start:08x}, ZF={zero}"), code, flags, Flags::all(Preserved))
                .at(start).dispatch(target).preserve_flag_record());
        }
    }
    cases
}

test_cases!(signed_relative_jumps, jump_cases());
test_cases!(conditional_word_truncation, word_condition_cases());

#[rustfmt::skip]
fn completed_encoding_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, taken_when_zero_clear) in [
        (&[0xeb, 1][..], true), (&[0x66, 0xeb, 1], true), (&[0xe9, 1, 0, 0, 0], true), (&[0x66, 0xe9, 1, 0], true),
        (&[0x74, 1], false), (&[0x66, 0x74, 1], false), (&[0x0f, 0x84, 1, 0, 0, 0], false), (&[0x66, 0x0f, 0x84, 1, 0], false),
    ] {
        for zero in [false, true].into_iter().take(if taken_when_zero_clear { 1 } else { 2 }) {
            let target = if zero || taken_when_zero_clear { 0x2001 } else { 0x2000 };
            cases.push(Case::new(format!("successor absent after {code:02x?}, ZF={zero}"), code,
                Flags { cf: true, pf: true, af: true, zf: zero, sf: true, of: true }, Flags::all(Preserved))
                .at(0x2000 - code.len() as u32).dispatch(target).preserve_flag_record());
        }
    }
    cases
}
test_cases!(complete_branch_encodings, completed_encoding_cases());

#[rustfmt::skip]
fn publication_cases() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("SUB produces zero; JNE falls through")
            .initial_register(Eax, 1)
            .step(Step::new(&[0x83, 0xe8, 1], Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .step(Step::preserving_flags(&[0x75, 0xfb]).dispatch(0x1005)),
        Sequence::from_opaque_flags("SUB produces one; JNE branches back")
            .initial_register(Eax, 2)
            .step(Step::new(&[0x83, 0xe8, 1], Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }).register(Eax, 1))
            .step(Step::preserving_flags(&[0x75, 0xfb]).dispatch(0x1000)),
        Sequence::preserving_flags("memory fault before a terminating branch publishes completed instructions")
            .step(Step::preserving_flags(&[0xb8, 7, 0, 0, 0]).register(Eax, 7))
            .step(Step::preserving_flags(&[0x8b, 0x0d, 0, 0x40, 0, 0]).fault(0x4000, 0))
            .trailing_code(&[0xeb, 0x7f], 1),
    ]
}

test_sequences!(arithmetic_branches_and_publication, publication_cases());

const ENCODINGS: &[&[u8]] = &[
    &[0xeb, 1],
    &[0x66, 0xeb, 1],
    &[0xe9, 1, 0, 0, 0],
    &[0x66, 0xe9, 1, 0],
    &[0x74, 1],
    &[0x66, 0x74, 1],
    &[0x0f, 0x84, 1, 0, 0, 0],
    &[0x66, 0x0f, 0x84, 1, 0],
];

#[test]
fn branch_forms_require_the_displacement_and_end_the_block() {
    for &code in ENCODINGS {
        let complete = check_length(code);
        let trailing = [code, &[0xb8, 0, 0, 0, 0, 0xf4]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, 99)
                .unwrap()
                .bytes,
            complete.bytes
        );
    }
}

#[test]
fn untaken_conditions_still_fetch_the_displacement() {
    for code in [
        &[0x74][..],
        &[0x0f, 0x84, 1, 0, 0][..],
        &[0x66, 0x0f, 0x84, 1][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        for zero in [0, 1] {
            let mut image = Image::new(&[]);
            image.cpu.eip = start;
            image.cpu.flags.status_source.kind = 0;
            image.cpu.flags.bytes.zf = zero;
            image.data(0x3000 + (start & 0xfff), code);
            image.check_unchanged_exit(
                Engine::Wasmtime,
                TestModule::interpreter(),
                &format!("incomplete branch {code:02x?}, ZF={zero}"),
                Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            );
        }
    }
}

#[test]
fn snapshot_limits_preserve_the_prefix_and_stop_at_the_first_branch() {
    let prefix = [0xb8, 0x78, 0x56, 0x34, 0x12];
    for branch in [
        &[0xeb, 0x7f][..],
        &[0x74, 0x7f],
        &[0x0f, 0x84, 0x7f, 0, 0, 0],
    ] {
        let code = [&prefix[..], branch].concat();
        let complete = compile_block_from_bytes(0x1000, &code, 2).unwrap();
        let trailing = [&code[..], &[0xf4, 0x66, 0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, u32::MAX)
                .unwrap()
                .bytes,
            complete.bytes
        );
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, 1)
                .unwrap()
                .bytes,
            compile_block_from_bytes(0x1000, &prefix, 1).unwrap().bytes
        );
    }
}
