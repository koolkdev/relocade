//! XCHG captures both old operands and commits them together.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::{self, *};

#[rustfmt::skip]
fn byte_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("XCHG AL,AH: shared parent", &[0x86, 0xc4])
            .register(Eax, 0x4433_2211, 0x4433_1122),
        Case::preserving_flags("XCHG CL,DL: low bytes", &[0x86, 0xca])
            .register(Ecx, 0x8877_6655, 0x8877_6699).register(Edx, 0xccbb_aa99, 0xccbb_aa55),
        Case::preserving_flags("XCHG AH,DH: high bytes", &[0x86, 0xe6])
            .register(Eax, 0x4433_2211, 0x4433_aa11).register(Edx, 0xccbb_aa99, 0xccbb_2299),
        Case::preserving_flags("XCHG DH,AL: override retains byte width", &[0x66, 0x86, 0xf0])
            .register(Edx, 0xccbb_aa99, 0xccbb_1199).register(Eax, 0x4433_2211, 0x4433_22aa),
        Case::preserving_flags("XCHG BH,BH: self exchange", &[0x86, 0xff])
            .initial_register(Ebx, 0x10ff_eedd),
    ]
}
test_cases!(
    byte_register_forms_exchange_old_values_including_shared_parents,
    byte_cases()
);

#[rustfmt::skip]
fn accumulator_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, other, input, eax, exchanged) in [
        (&[0x90][..], Eax, 0x4433_2211, 0x4433_2211, 0x4433_2211),
        (&[0x66, 0x90], Eax, 0x4433_2211, 0x4433_2211, 0x4433_2211),
        (&[0x91], Ecx, 0x8877_6655, 0x8877_6655, 0x4433_2211),
        (&[0x66, 0x92], Edx, 0xccbb_aa99, 0x4433_aa99, 0xccbb_2211),
        (&[0x93], Ebx, 0x10ff_eedd, 0x10ff_eedd, 0x4433_2211),
        (&[0x66, 0x94], Esp, 0x5555_5555, 0x4433_5555, 0x5555_2211),
        (&[0x95], Ebp, 0x6666_6666, 0x6666_6666, 0x4433_2211),
        (&[0x66, 0x96], Esi, 0x7777_7777, 0x4433_7777, 0x7777_2211),
        (&[0x97], Edi, 0x8888_8888, 0x8888_8888, 0x4433_2211),
    ] {
        let mut case = Case::preserving_flags(format!("accumulator exchange {code:02x?}"), code)
            .register(Eax, 0x4433_2211, eax);
        if other != Eax { case = case.register(other, input, exchanged); }
        cases.push(case);
    }
    cases
}
test_cases!(compact_opcodes_and_nop_aliases, accumulator_cases());

#[rustfmt::skip]
fn general_register_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("XCHG SI,BP: upper halves preserved", &[0x66, 0x87, 0xf5])
            .register(Esi, 0x7777_7777, 0x7777_6666).register(Ebp, 0x6666_6666, 0x6666_7777),
        Case::preserving_flags("XCHG EDI,EAX: all four bytes", &[0x87, 0xf8])
            .register(Edi, 0x8888_8888, 0x4433_2211).register(Eax, 0x4433_2211, 0x8888_8888),
        Case::preserving_flags("XCHG ESP,ESP: self exchange", &[0x87, 0xe4])
            .initial_register(Esp, 0x5555_5555),
    ]
}
test_cases!(
    general_word_and_dword_register_forms_preserve_unwritten_bits,
    general_register_cases()
);

#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("AH exchange uses only the last readable byte", &[0x86, 0x23])
            .initial_register(Ebx, 0x4fff).register(Eax, 0x4433_2211, 0x4433_8011)
            .memory(0x4ffe, &[0x5a, 0x80], ReadWrite).expect_memory(0x4fff, &[0x22]),
        Case::preserving_flags("word exchange keeps both upper register bytes", &[0x66, 0x87, 0x3b])
            .initial_register(Ebx, 0x4ffe).register(Edi, 0x8888_8888, 0x8888_fe80)
            .memory(0x4ffd, &[0x5a, 0x80, 0xfe], ReadWrite).expect_memory(0x4ffe, &[0x88, 0x88]),
        Case::preserving_flags("dword exchange spans scattered pages", &[0x87, 0x03])
            .initial_register(Ebx, 0x4ffe).register(Eax, 0x4433_2211, 0x9234_5678)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[0x5a, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite)
            .expect_memory(0x4ffe, &[0x11, 0x22, 0x33, 0x44]),
    ]
}
test_cases!(memory_widths_and_both_results, memory_cases());

#[rustfmt::skip]
fn address_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    struct Exchange {
        name: &'static str,
        code: &'static [u8],
        register: Gpr32,
        old: u32,
        result: u32,
        stored_bytes: &'static [u8],
    }
    for case in [
        Exchange {
            name: "dword exchange uses its old EAX base",
            code: &[0x87, 0x00],
            register: Eax,
            old: 0x8000_4010,
            result: 0x9234_5678,
            stored_bytes: &[0x10, 0x40, 0, 0x80],
        },
        Exchange {
            name: "word exchange retains the full old EAX base",
            code: &[0x66, 0x87, 0x00],
            register: Eax,
            old: 0x8000_4010,
            result: 0x8000_5678,
            stored_bytes: &[0x10, 0x40],
        },
        Exchange {
            name: "AH exchange cannot change its EAX base early",
            code: &[0x86, 0x20],
            register: Eax,
            old: 0x8000_4010,
            result: 0x8000_7810,
            stored_bytes: &[0x40],
        },
        Exchange {
            name: "scaled ECX index is evaluated before ECX changes",
            code: &[0x87, 0x4c, 0x8b, 0x10],
            register: Ecx,
            old: 4,
            result: 0x9234_5678,
            stored_bytes: &[4, 0, 0, 0],
        },
    ] {
        cases.push(Case::preserving_flags(case.name, case.code)
            .initial_register(Ebx, 0x8000_3ff0).register(case.register, case.old, case.result)
            .memory(0x8000_400f, &[0x5a, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite)
            .expect_memory(0x8000_4010, case.stored_bytes));
    }
    cases
}
test_cases!(
    exchanged_base_and_index_registers_use_the_old_effective_address,
    address_cases()
);

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("missing byte operand is a write fault", &[0x86, 0x03])
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
        Case::preserving_flags("equal word values still require write permission", &[0x66, 0x87, 0x03])
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x4020)])
            .memory(0x4020, &[0x11, 0x22], ReadOnly).fault(0x4020, 3),
        Case::preserving_flags("missing second page preserves both word operands", &[0x66, 0x87, 0x03])
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x4fff)])
            .memory(0x4fff, &[0x80], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("read-only second page preserves both dword operands", &[0x87, 0x03])
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x4ffe)])
            .memory(0x4ffe, &[0x80, 0xfe], ReadWrite).memory(0x5000, &[0xdc, 0xba], ReadOnly)
            .fault(0x5000, 3),
    ]
}
test_cases!(faults_preserve_both_operands, fault_cases());

#[test]
fn forms_consume_no_immediate() {
    for code in [
        &[0x90][..],
        &[0x66, 0x97],
        &[0x86, 0xc4],
        &[0x87, 0xc1],
        &[0x66, 0x87, 0xf5],
        &[0x86, 0x44, 0x8b, 0x80],
        &[0x66, 0x87, 0x04, 0x25, 0x20, 0x40, 0, 0],
    ] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn sequences() -> Vec<Sequence> {
    vec![Sequence::from_opaque_flags("exchanges preserve pending arithmetic and publish both operands before a fault")
        .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0xccbb_aa99), (Ebx, 0x4000), (Ebp, 0)])
        .map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite)
        .backing(0x7fff, &[0x5a, 0x10, 0x40, 0, 0, 0x5a]).backing(0x800f, &[0x5a, 0x80, 0x5a])
        .step(Step::new(&[0x83, 0xed, 1],
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Ebp, u32::MAX))
        .step(Step::preserving_flags(&[0x86, 0xc4]).register(Eax, 0x4433_1122))
        .step(Step::preserving_flags(&[0x66, 0x92]).register(Eax, 0x4433_aa99).register(Edx, 0xccbb_1122))
        .step(Step::preserving_flags(&[0x87, 0x03]).register(Eax, 0x0000_4010).expect_memory(0x4000, &[0x99, 0xaa, 0x33, 0x44]))
        .step(Step::preserving_flags(&[0x86, 0x20]).register(Eax, 0x0000_8010).expect_memory(0x4010, &[0x40]))
        .step(Step::preserving_flags(&[0x66, 0x87, 0x11]).register(Edx, 0xccbb_aa99).expect_memory(0x6000, &[0x22, 0x11]))
        .step(Step::preserving_flags(&[0x87, 0x00]).fault(0x8010, 2))]
}

test_sequences!(aliases_and_publication, sequences());
