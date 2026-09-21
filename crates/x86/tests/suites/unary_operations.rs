//! Unary arithmetic boundaries, carry preservation and read/modify/write effects.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError, CpuState,
    Gpr32::{self, Eax, Ebx, Ecx, Edi, Edx, Esi},
    StoredStatusSource,
};

// Input, result and parity columns are byte, word and dword respectively.
struct Boundary {
    name: &'static str,
    input: [u32; 3],
    result: [u32; 3],
    carry: FlagExpectation,
    parity: [FlagExpectation; 3],
    auxiliary: FlagExpectation,
    zero: FlagExpectation,
    sign: FlagExpectation,
    overflow: FlagExpectation,
}

#[rustfmt::skip]
const INCREMENTS: &[Boundary] = &[
    Boundary { name: "zero to one", input: [0x1234_5600, 0x1234_0000, 0], result: [0x1234_5601, 0x1234_0001, 1],
        carry: Preserved, parity: [Clear; 3], auxiliary: Clear, zero: Clear, sign: Clear, overflow: Clear },
    Boundary { name: "nibble carry", input: [0x1234_560f, 0x1234_000f, 0x0f], result: [0x1234_5610, 0x1234_0010, 0x10],
        carry: Preserved, parity: [Clear; 3], auxiliary: Set, zero: Clear, sign: Clear, overflow: Clear },
    Boundary { name: "signed overflow", input: [0x1234_567f, 0x1234_7fff, 0x7fff_ffff], result: [0x1234_5680, 0x1234_8000, 0x8000_0000],
        carry: Preserved, parity: [Clear, Set, Set], auxiliary: Set, zero: Clear, sign: Set, overflow: Set },
    Boundary { name: "unsigned wrap", input: [0x1234_56ff, 0x1234_ffff, 0xffff_ffff], result: [0x1234_5600, 0x1234_0000, 0],
        carry: Preserved, parity: [Set; 3], auxiliary: Set, zero: Set, sign: Clear, overflow: Clear },
];

#[rustfmt::skip]
const DECREMENTS: &[Boundary] = &[
    Boundary { name: "one to zero", input: [0x1234_5601, 0x1234_0001, 1], result: [0x1234_5600, 0x1234_0000, 0],
        carry: Preserved, parity: [Set; 3], auxiliary: Clear, zero: Set, sign: Clear, overflow: Clear },
    Boundary { name: "nibble borrow", input: [0x1234_5610, 0x1234_0010, 0x10], result: [0x1234_560f, 0x1234_000f, 0x0f],
        carry: Preserved, parity: [Set; 3], auxiliary: Set, zero: Clear, sign: Clear, overflow: Clear },
    Boundary { name: "signed overflow", input: [0x1234_5680, 0x1234_8000, 0x8000_0000], result: [0x1234_567f, 0x1234_7fff, 0x7fff_ffff],
        carry: Preserved, parity: [Clear, Set, Set], auxiliary: Set, zero: Clear, sign: Clear, overflow: Set },
    Boundary { name: "unsigned wrap", input: [0x1234_5600, 0x1234_0000, 0], result: [0x1234_56ff, 0x1234_ffff, 0xffff_ffff],
        carry: Preserved, parity: [Set; 3], auxiliary: Set, zero: Clear, sign: Set, overflow: Clear },
];

#[rustfmt::skip]
const NEGATIONS: &[Boundary] = &[
    Boundary { name: "zero clears carry", input: [0x1234_5600, 0x1234_0000, 0], result: [0x1234_5600, 0x1234_0000, 0],
        carry: Clear, parity: [Set; 3], auxiliary: Clear, zero: Set, sign: Clear, overflow: Clear },
    Boundary { name: "one to minus one", input: [0x1234_5601, 0x1234_0001, 1], result: [0x1234_56ff, 0x1234_ffff, u32::MAX],
        carry: Set, parity: [Set; 3], auxiliary: Set, zero: Clear, sign: Set, overflow: Clear },
    Boundary { name: "no nibble borrow", input: [0x1234_5610, 0x1234_0010, 0x10], result: [0x1234_56f0, 0x1234_fff0, 0xffff_fff0],
        carry: Set, parity: [Set; 3], auxiliary: Clear, zero: Clear, sign: Set, overflow: Clear },
    Boundary { name: "positive signed maximum", input: [0x1234_567f, 0x1234_7fff, 0x7fff_ffff], result: [0x1234_5681, 0x1234_8001, 0x8000_0001],
        carry: Set, parity: [Set, Clear, Clear], auxiliary: Set, zero: Clear, sign: Set, overflow: Clear },
    Boundary { name: "signed minimum overflows", input: [0x1234_5680, 0x1234_8000, 0x8000_0000], result: [0x1234_5680, 0x1234_8000, 0x8000_0000],
        carry: Set, parity: [Clear, Set, Set], auxiliary: Clear, zero: Clear, sign: Set, overflow: Set },
    Boundary { name: "minus one to one", input: [0x1234_56ff, 0x1234_ffff, u32::MAX], result: [0x1234_5601, 0x1234_0001, 1],
        carry: Set, parity: [Clear; 3], auxiliary: Set, zero: Clear, sign: Clear, overflow: Clear },
];

fn arithmetic_boundaries() -> Vec<Case> {
    let mut cases = Vec::new();
    for (operation, byte_opcode, wide_opcode, modrm, boundaries) in [
        ("INC", 0xfe, 0xff, 0xc0, INCREMENTS),
        ("DEC", 0xfe, 0xff, 0xc8, DECREMENTS),
        ("NEG", 0xf6, 0xf7, 0xd8, NEGATIONS),
    ] {
        for (column, code) in [
            vec![byte_opcode, modrm],
            vec![0x66, wide_opcode, modrm],
            vec![wide_opcode, modrm],
        ]
        .into_iter()
        .enumerate()
        {
            for (index, boundary) in boundaries.iter().enumerate() {
                let name = format!("{operation} {}-bit: {}", 8 << column, boundary.name);
                let expected = Flags {
                    cf: boundary.carry,
                    pf: boundary.parity[column],
                    af: boundary.auxiliary,
                    zf: boundary.zero,
                    sf: boundary.sign,
                    of: boundary.overflow,
                };
                let case = match boundary.carry {
                    Preserved => Case::new(name, &code, Flags::all(index % 2 != 0), expected),
                    _ => Case::replacing_flags(name, &code, expected),
                };
                cases.push(case.register(Eax, boundary.input[column], boundary.result[column]));
            }
        }
    }
    cases
}
test_cases!(arithmetic_boundaries_and_carry, arithmetic_boundaries());

#[rustfmt::skip]
fn register_forms() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags("NOT AL preserves opaque flags", &[0xf6, 0xd0])
            .register(Eax, 0x1234_5600, 0x1234_56ff),
        Case::preserving_flags("NOT AX preserves opaque flags", &[0x66, 0xf7, 0xd0])
            .register(Eax, 0x1234_0000, 0x1234_ffff),
        Case::preserving_flags("NOT EAX preserves opaque flags", &[0xf7, 0xd0])
            .register(Eax, 0x1234_5678, 0xedcb_a987),
        Case::new("INC AH keeps AL and the upper word", &[0xfe, 0xc4], Flags::all(true),
            Flags { cf: Preserved, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x9234_8080, 0x9234_8180),
        Case::new("DEC DH keeps DL and the upper word", &[0xfe, 0xce], Flags::all(false),
            Flags { cf: Preserved, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Set })
            .register(Edx, 0x9234_8080, 0x9234_7f80),
        Case::preserving_flags("NOT CH changes only the high byte", &[0xf6, 0xd5])
            .register(Ecx, 0x9234_8080, 0x9234_7f80),
        Case::replacing_flags("NEG BH overflows in byte width", &[0xf6, 0xdf],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Ebx, 0x9234_8080, 0x9234_8080),
    ];
    for (opcode, inputs, outputs, flags) in [
        (0x40, [u32::MAX, 0x9234_ffff], [0, 0x9234_0000],
            Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }),
        (0x48, [0, 0x9234_0000], [u32::MAX, 0x9234_ffff],
            Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }),
    ] {
        for (index, register) in Gpr32::ALL.into_iter().enumerate() {
            let word = index % 2;
            let mut code = if word == 1 { vec![0x66] } else { vec![] };
            code.push(opcode + index as u8);
            cases.push(Case::new(format!("compact unary {code:02x?}"), &code, Flags::all(true), flags)
                .register(register, inputs[word], outputs[word]));
        }
    }
    cases
}
test_cases!(compact_encodings_and_byte_aliases, register_forms());

#[rustfmt::skip]
fn stored_carry_cases() -> Vec<Case> {
    [
        (Case::new("INC reads carry from pending ADD", &[0x40],
            Flags { cf: true, pf: true, af: true, zf: true, sf: false, of: false },
            Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x7fff_ffff, 0x8000_0000), 2, 0xff, 0),
        (Case::new("DEC reads carry from pending SUB", &[0x48], Flags::all(false),
            Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x7fff_ffff), 9, 3, 1),
    ]
    .into_iter()
    .map(|(case, kind, left, concrete_carry)| {
        let mut record = CpuState::filled(0xa5).flags;
        record.status_source = StoredStatusSource { kind, left, right: 1, ..record.status_source };
        record.bytes.cf = concrete_carry;
        case.stored_flags(record)
    })
    .collect()
}
test_cases!(
    stored_arithmetic_overrides_the_concrete_carry_byte,
    stored_carry_cases()
);

#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::new("INC byte uses only the last mapped byte", &[0xfe, 0x03], Flags::all(true),
            Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .initial_register(Ebx, 0x4fff).memory(0x4ffe, &[0x5a, 0xff], ReadWrite).expect_memory(0x4fff, &[0]),
        Case::new("DEC word updates both scattered bytes", &[0x66, 0xff, 0x0b], Flags::all(false),
            Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .initial_register(Ebx, 0x4fff).map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffe, &[0x5a, 0, 0, 0x5a], ReadWrite).expect_memory(0x4fff, &[0xff, 0xff]),
        Case::preserving_flags("NOT dword preserves opaque flags while storing", &[0xf7, 0x13])
            .initial_register(Ebx, 0x4010).memory(0x400f, &[0x5a, 0x0f, 0x0f, 0x0f, 0x0f, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[0xf0, 0xf0, 0xf0, 0xf0]),
        Case::replacing_flags("NEG dword replaces incoming flags after a split read", &[0xf7, 0x1b],
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0x5a, 1, 0, 0, 0, 0x5a], ReadWrite)
            .expect_memory(0x4ffe, &[0xff; 4]),
        Case::preserving_flags("INC missing byte leaves flags unchanged", &[0xfe, 0x03])
            .initial_register(Ebx, 0x4000).fault(0x4000, 2),
        Case::preserving_flags("DEC missing second word byte cannot partially store", &[0x66, 0xff, 0x0b])
            .initial_register(Ebx, 0x4fff).memory(0x4fff, &[0], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("NOT read-only second page prevents the whole store", &[0xf7, 0x13])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[0, 0], ReadWrite)
            .memory(0x5000, &[0, 0], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("NEG zero still requires write access", &[0xf6, 0x1b])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0], ReadOnly).fault(0x4000, 3),
    ]
}
test_cases!(memory_effects_and_atomic_faults, memory_cases());

#[rustfmt::skip]
fn dependent_flags() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("INC and DEC preserve pending ADD carry for ADC and SBB")
            .initial_registers(&[(Eax, u32::MAX), (Ebx, 0), (Ecx, u32::MAX), (Esi, u32::MAX), (Edi, 0)])
            .step(Step::new(&[0x83, 0xc1, 1], Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Ecx, 0))
            .step(Step::new(&[0x40], Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .step(Step::new(&[0x4b], Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Ebx, u32::MAX))
            .step(Step::new(&[0x83, 0xd6, 0], Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Esi, 0))
            .step(Step::new(&[0x83, 0xdf, 0], Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Edi, u32::MAX)),
        Sequence::new("INC retains locally cleared carry while replacing ZF", Flags::all(true))
            .initial_registers(&[(Eax, 0x1234_56ff), (Ecx, 3), (Edx, 0xffff_ffff)])
            .step(Step::new(&[0x83, 0xe9, 1], Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }).register(Ecx, 2))
            .step(Step::new(&[0xfe, 0xc0], Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x1234_5600))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc2]).register(Edx, 0xffff_ff00))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc6]).register(Edx, 0xffff_0100)),
        Sequence::from_opaque_flags("NOT preserves pending NEG overflow and carry for consumers")
            .initial_registers(&[(Eax, 0x1234_5680), (Ecx, 0), (Edx, 0)])
            .step(Step::new(&[0xf6, 0xd8], Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set }))
            .step(Step::preserving_flags(&[0x66, 0xf7, 0xd0]).register(Eax, 0x1234_a97f))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc1]).register(Ecx, 1))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc2]).register(Edx, 1)),
        Sequence::new("mixed-width stores publish before a later unary write fault", Flags::all(true))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x6000)])
            .memory(0x4ffe, &[0x5a, 0xff, 0, 0x5a], ReadWrite)
            .step(Step::new(&[0xfe, 0x03], Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).expect_memory(0x4fff, &[0]))
            .step(Step::new(&[0x66, 0xff, 0x0b], Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).expect_memory(0x4fff, &[0xff, 0xff]))
            .step(Step::preserving_flags(&[0xff, 0x01]).fault(0x6000, 2)),
    ]
}
test_sequences!(
    pending_flags_consumers_and_fault_publication,
    dependent_flags()
);

#[test]
fn unary_lengths_stop_after_the_selected_register_or_address() {
    for code in [
        &[0x40][..],
        &[0x4f],
        &[0x66, 0x43],
        &[0x66, 0x66, 0x4c],
        &[0xfe, 0xc4],
        &[0x66, 0xfe, 0xcc],
        &[0xff, 0xc0],
        &[0x66, 0xff, 0xc8],
        &[0xf6, 0xd0],
        &[0x66, 0xf6, 0xdc],
        &[0xf7, 0xd0],
        &[0x66, 0xf7, 0xd8],
        &[0xfe, 0x44, 0x8b, 0x80],
        &[0xff, 0x84, 0x8b, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xff, 0x0d, 0x11, 0x22, 0x33, 0x44],
        &[0xf6, 0x54, 0x8b, 0x80],
        &[0xf7, 0x14, 0x25, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xf7, 0x9c, 0x8b, 0x11, 0x22, 0x33, 0x44],
        // The same F6/F7 opcodes still consume an immediate for TEST /0.
        &[0xf6, 0xc0, 0xf7],
        &[0xf7, 0xc0, 0xf6, 0xf7, 0xfe, 0xff],
        &[0x66, 0xf7, 0xc0, 0xf6, 0xf7],
    ] {
        check_length(code);
    }
}

#[test]
fn unsupported_group_extensions_stop_before_sib_or_displacement_fetch() {
    for (opcode, modrm) in [(0xfe, 0x14), (0xff, 0x3c), (0xf6, 0x0c), (0xf7, 0x0d)] {
        let code = [opcode, modrm];
        let start = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(start, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: start,
                opcode
            }),
        );
        let mut image = Image::new(&[]);
        image.cpu.flags.status_source.kind = 0xff;
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "unsupported extension precedes address fields",
            Exit::Other(0x0008_0000_0000_0000 | (u64::from(opcode) << 32) | u64::from(start)),
        );
    }
}
