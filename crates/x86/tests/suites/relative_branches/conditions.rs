use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set, Undefined},
        Flags, InstructionCase,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::{FlagBytes, StoredStatusSource};
use wasm86_x86::{
    Gpr32::{self, Eax, Ebx},
    StoredFlags,
};

// Outcome order: O, NO, B, AE, E, NE, BE, A, S, NS, P, NP, L, GE, LE, G.
const CLEAR_INPUTS: [u8; 16] = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
const EQUAL: [u8; 16] = [0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0];
const CARRY_OVERFLOW: [u8; 16] = [1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0];
const NEGATIVE: [u8; 16] = [0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0];
const SIGNED_UNSIGNED_DISAGREE: [u8; 16] = [1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1];
const ADC_OVERFLOW: [u8; 16] = [1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1];

struct Form {
    code: &'static [u8],
    condition_byte: usize,
}
#[rustfmt::skip]
const FORMS: [Form; 3] = [
    Form { code: &[0x70, 0x7f], condition_byte: 0 },
    Form { code: &[0x0f, 0x80, 0x7f, 0, 0, 0], condition_byte: 1 },
    Form { code: &[0x66, 0x0f, 0x80, 0x7f, 0], condition_byte: 2 },
];

#[derive(Clone, Copy)]
struct Targets {
    taken: u32,
    fallthrough: u32,
}
#[rustfmt::skip]
const AT_ENTRY: [Targets; 3] = [
    Targets { taken: 0x1081, fallthrough: 0x1002 },
    Targets { taken: 0x1085, fallthrough: 0x1006 },
    Targets { taken: 0x1084, fallthrough: 0x1005 },
];
#[rustfmt::skip]
const AFTER_TWO_BYTES: [Targets; 3] = [
    Targets { taken: 0x1083, fallthrough: 0x1004 },
    Targets { taken: 0x1087, fallthrough: 0x1008 },
    Targets { taken: 0x1086, fallthrough: 0x1007 },
];
#[rustfmt::skip]
const AFTER_THREE_BYTES: [Targets; 3] = [
    Targets { taken: 0x1084, fallthrough: 0x1005 },
    Targets { taken: 0x1088, fallthrough: 0x1009 },
    Targets { taken: 0x1087, fallthrough: 0x1008 },
];

const STORED_CANARY: StoredFlags = StoredFlags {
    status_source: StoredStatusSource {
        kind: 0,
        reserved: [0xa5; 3],
        left: 0xa5a5_a5a5,
        right: 0xa5a5_a5a5,
    },
    bytes: FlagBytes {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
        tf: 0,
        df: 1,
        nt: 0,
        ac: 0,
        id: 0,
        reserved: 0xa5,
    },
};

#[rustfmt::skip]
fn input_cases() -> Vec<InstructionCase> {
    struct Input { name: &'static str, flags: Flags<bool>, stored: Option<StoredFlags>, taken: [u8; 16] }
    let inputs = [
        Input { name: "all condition inputs clear", flags: Flags { cf: false, pf: false, af: true, zf: false, sf: false, of: false }, stored: None, taken: CLEAR_INPUTS },
        Input { name: "equal without carry", flags: Flags { cf: false, pf: true, af: true, zf: true, sf: false, of: false }, stored: None, taken: EQUAL },
        Input { name: "carry and overflow", flags: Flags { cf: true, pf: true, af: true, zf: true, sf: false, of: true }, stored: None, taken: CARRY_OVERFLOW },
        Input { name: "negative without overflow", flags: Flags { cf: false, pf: false, af: true, zf: false, sf: true, of: false }, stored: None, taken: NEGATIVE },
        Input { name: "stored ADD carry and overflow", flags: Flags { cf: true, pf: true, af: false, zf: true, sf: false, of: true },
            stored: Some(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 10,
                    left: 0x8000_0000,
                    right: 0x8000_0000,
                    ..STORED_CANARY.status_source
                },
                ..STORED_CANARY
            }), taken: CARRY_OVERFLOW },
        Input { name: "stored SUB signed and unsigned disagreement", flags: Flags { cf: true, pf: true, af: false, zf: false, sf: true, of: true },
            stored: Some(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 9,
                    left: 0x7fff_fffe,
                    right: 0xffff_fffe,
                    ..STORED_CANARY.status_source
                },
                ..STORED_CANARY
            }), taken: SIGNED_UNSIGNED_DISAGREE },
        Input { name: "stored logical odd result", flags: Flags { cf: false, pf: false, af: false, zf: false, sf: false, of: false },
            stored: Some(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 11,
                    left: 1,
                    right: 0x1234_5678,
                    ..STORED_CANARY.status_source
                },
                ..STORED_CANARY
            }), taken: CLEAR_INPUTS },
        Input { name: "stored logical zero result", flags: Flags { cf: false, pf: true, af: false, zf: true, sf: false, of: false },
            stored: Some(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 11,
                    left: 0,
                    right: 0x1234_5678,
                    ..STORED_CANARY.status_source
                },
                ..STORED_CANARY
            }), taken: EQUAL },
    ];
    let mut cases = Vec::new();
    for input in inputs {
        for (condition, taken) in input.taken.into_iter().enumerate() {
            for (form, targets) in FORMS.iter().zip(AT_ENTRY) {
                let mut code = form.code.to_vec();
                code[form.condition_byte] += condition as u8;
                let mut case = InstructionCase::new(format!("{} via {code:02x?}", input.name), &code, input.flags, Flags::all(Preserved))
                    .dispatch(if taken == 1 { targets.taken } else { targets.fallthrough }).preserve_flag_record();
                if let Some(record) = input.stored { case = case.stored_flags(record); }
                cases.push(case);
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn locally_produced_cases() -> Vec<SequenceCase> {
    struct Producer {
        name: &'static str, code: &'static [u8], inputs: &'static [(Gpr32, u32)], outputs: &'static [(Gpr32, u32)],
        flags: Flags<FlagExpectation>, taken: [u8; 16], targets: [Targets; 3],
    }
    let producers = [
        Producer { name: "local ADD", code: &[0x01, 0xc0], inputs: &[(Eax, 0x8000_0000)], outputs: &[(Eax, 0)],
            flags: Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set }, taken: CARRY_OVERFLOW, targets: AFTER_TWO_BYTES },
        Producer { name: "local CMP", code: &[0x39, 0xd8], inputs: &[(Eax, 0x7fff_fffe), (Ebx, 0xffff_fffe)], outputs: &[],
            flags: Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set }, taken: SIGNED_UNSIGNED_DISAGREE, targets: AFTER_TWO_BYTES },
        Producer { name: "local TEST odd result", code: &[0x85, 0xc0], inputs: &[(Eax, 1)], outputs: &[],
            flags: Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Clear, of: Clear }, taken: CLEAR_INPUTS, targets: AFTER_TWO_BYTES },
        Producer { name: "local TEST zero result", code: &[0x85, 0xc0], inputs: &[(Eax, 0)], outputs: &[],
            flags: Flags { cf: Clear, pf: Set, af: Undefined, zf: Set, sf: Clear, of: Clear }, taken: EQUAL, targets: AFTER_TWO_BYTES },
        Producer { name: "local ADC explicit flags", code: &[0x83, 0xd0, 0], inputs: &[(Eax, 0x7fff_ffff)], outputs: &[(Eax, 0x8000_0000)],
            flags: Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }, taken: ADC_OVERFLOW, targets: AFTER_THREE_BYTES },
    ];
    let mut cases = Vec::new();
    for producer in producers {
        for (condition, taken) in producer.taken.into_iter().enumerate() {
            for (form, targets) in FORMS.iter().zip(producer.targets) {
                let mut branch = form.code.to_vec();
                branch[form.condition_byte] += condition as u8;
                let mut first = Checkpoint::new(producer.code, producer.flags);
                for &(register, output) in producer.outputs { first = first.register(register, output); }
                cases.push(SequenceCase::new(format!("{} then {branch:02x?}", producer.name), Flags::all(true))
                    .initial_registers(producer.inputs).step(first)
                    .step(Checkpoint::preserving_flags(&branch).dispatch(if taken == 1 { targets.taken } else { targets.fallthrough })));
            }
        }
    }
    cases
}

test_cases!(concrete_and_stored_flags, input_cases());
test_sequences!(locally_created_flags, locally_produced_cases());
