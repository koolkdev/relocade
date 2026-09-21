//! XADD and CMPXCHG old operands, comparison outcomes and precise faults.
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
use wasm86_x86::Gpr32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};

#[rustfmt::skip]
fn xadd_boundaries() -> Vec<Case> {
    vec![
        Case::replacing_flags("byte carry and signed overflow", &[0x0f, 0xc0, 0xd8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x4433_2280, 0x4433_2200).initial_register(Ebx, 0x10ff_ee80),
        Case::replacing_flags("byte auxiliary carry with odd low-byte parity", &[0x0f, 0xc0, 0xd8],
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_220f, 0x4433_2210).register(Ebx, 0x10ff_ee01, 0x10ff_ee0f),
        Case::replacing_flags("word signed overflow with even low-byte parity", &[0x66, 0x0f, 0xc1, 0xd8],
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7fff, 0x4433_8000).register(Ebx, 0x10ff_0001, 0x10ff_7fff),
        Case::replacing_flags("dword carry and zero without signed overflow", &[0x0f, 0xc1, 0xd8],
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, u32::MAX, 0).register(Ebx, 1, u32::MAX),
    ]
}
test_cases!(xadd_arithmetic_boundaries, xadd_boundaries());

#[rustfmt::skip]
fn xadd_registers() -> Vec<Case> {
    vec![
        Case::replacing_flags("XADD AL,AH ignores the operand-size prefix", &[0x66, 0x0f, 0xc0, 0xe0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_1133),
        Case::replacing_flags("XADD AH,AL reads both old aliases", &[0x0f, 0xc0, 0xc4],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_3322),
        Case::replacing_flags("XADD AH,AH keeps the destination sum", &[0x0f, 0xc0, 0xe4],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_4411),
        Case::replacing_flags("XADD AX,AX keeps its upper half", &[0x66, 0x0f, 0xc1, 0xc0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_4422),
        Case::replacing_flags("XADD EAX,EAX carries and overflows", &[0x0f, 0xc1, 0xc0],
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0),
        Case::replacing_flags("XADD ESI,EBP returns old ESI to EBP", &[0x0f, 0xc1, 0xee],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Esi, 0x7777_7777, 0xdddd_dddd)
            .register(Ebp, 0x6666_6666, 0x7777_7777),
        Case::replacing_flags("XADD SP,DI keeps both upper halves", &[0x66, 0x0f, 0xc1, 0xfc],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Esp, 0x5555_5555, 0x5555_dddd)
            .register(Edi, 0x8888_8888, 0x8888_5555),
    ]
}

#[rustfmt::skip]
fn xadd_addresses() -> Vec<Case> {
    vec![
        Case::replacing_flags("XADD [EAX],EAX keeps the old address base", &[0x0f, 0xc1, 0x00],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4010, 1).initial_register(Ebx, 0x3ff0)
            .memory(0x400f, &[0x5a, 1, 0, 0, 0, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[0x11, 0x40, 0, 0]),
        Case::replacing_flags("XADD [EAX],AH keeps the old address parent", &[0x0f, 0xc0, 0x20],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4010, 0x8010).initial_register(Ebx, 0x3ff0)
            .memory(0x400f, &[0x5a, 0x80, 0x5a, 0x5a, 0x5a, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[0xc0]),
        Case::replacing_flags("XADD word [EBX+ECX*4+16],CX uses the old scaled index", &[0x66, 0x0f, 0xc1, 0x4c, 0x8b, 0x10],
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8000_0004, 0x8000_ffff).initial_register(Ebx, 0x3ff0)
            .memory(0x400f, &[0x5a, 0xff, 0xff, 0x5a, 0x5a, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[3, 0]),
    ]
}

test_cases!(xadd_register_aliases, xadd_registers());
test_cases!(xadd_address_aliases, xadd_addresses());
#[rustfmt::skip]
fn cmpxchg_registers() -> Vec<Case> {
    vec![
        Case::replacing_flags("byte mismatch replaces AL with old CL", &[0x0f, 0xb0, 0xd1],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2255).initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("byte match replaces CL with DL", &[0x0f, 0xb0, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_6611, 0x8877_6699).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("word mismatch preserves the upper accumulator half", &[0x66, 0x0f, 0xb1, 0xd1],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_6655).initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("word match preserves the upper destination half", &[0x66, 0x0f, 0xb1, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_2211, 0x8877_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("dword mismatch replaces EAX with old ECX", &[0x0f, 0xb1, 0xd1],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_2211, 0x8877_6655).initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("dword match replaces ECX with EDX", &[0x0f, 0xb1, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x4433_2211, 0xccbb_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("AL destination takes DL", &[0x0f, 0xb0, 0xd0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2299).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("AX destination takes DX", &[0x66, 0x0f, 0xb1, 0xd0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("EAX destination takes EDX", &[0x0f, 0xb1, 0xd0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0xccbb_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::replacing_flags("AH mismatch replaces AL without replacing AH", &[0x0f, 0xb0, 0xcc],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2222).initial_register(Ecx, 0x8877_6655),
        Case::replacing_flags("AH match changes AH while preserving AL", &[0x0f, 0xb0, 0xcc],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_1111, 0x4433_5511).initial_register(Ecx, 0x8877_6655),
        Case::replacing_flags("AH source supplies its old value to matching CL", &[0x0f, 0xb0, 0xe1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_6611, 0x8877_6622),
        Case::replacing_flags("an accumulator source does not overwrite a mismatch destination", &[0x0f, 0xb1, 0xc1],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_2211, 0x8877_6655).initial_register(Ecx, 0x8877_6655),
        Case::replacing_flags("a matching source and destination still publish the comparison", &[0x0f, 0xb1, 0xc9],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).initial_register(Ecx, 0x4433_2211),
    ]
}

test_cases!(register_outcomes_and_aliases, cmpxchg_registers());

#[rustfmt::skip]
fn cmpxchg_addresses() -> Vec<Case> {
    vec![
        Case::replacing_flags("word mismatch keeps the old EAX address and upper half", &[0x66, 0x0f, 0xb1, 0x10],
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x1111_4020, 0x1111_fedc).initial_register(Edx, 0xccbb_aa99)
            .memory(0x1111_4020, &[0xdc, 0xfe], ReadWrite),
        Case::replacing_flags("matching memory destination takes its EBX address source", &[0x0f, 0xb1, 0x1b],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).initial_register(Ebx, 0x4020)
            .memory(0x4020, &[0x11, 0x22, 0x33, 0x44], ReadWrite).expect_memory(0x4020, &[0x20, 0x40, 0, 0]),
        Case::replacing_flags("matching memory destination takes its scaled ECX index source", &[0x0f, 0xb1, 0x4c, 0x8b, 0x20],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).initial_register(Ebx, 0x4000).initial_register(Ecx, 8)
            .memory(0x4040, &[0x11, 0x22, 0x33, 0x44], ReadWrite).expect_memory(0x4040, &[8, 0, 0, 0]),
    ]
}

test_cases!(cmpxchg_address_aliases, cmpxchg_addresses());

#[rustfmt::skip]
fn memory_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("XADD byte needs a writable operand", &[0x0f, 0xc0, 0x03])
            .initial_register(Ebx, 0x4000).fault(0x4000, 2),
        Case::preserving_flags("XADD fault preserves its old address/source register", &[0x0f, 0xc1, 0x00])
            .initial_register(Eax, 0x4000).memory(0x4000, &[1, 0, 0, 0], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("XADD word cannot partially write before a missing second byte", &[0x66, 0x0f, 0xc1, 0x03])
            .initial_registers(&[(Eax, 1), (Ebx, 0x4fff)])
            .memory(0x4fff, &[0xff], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("CMPXCHG mismatch still requires write permission", &[0x0f, 0xb0, 0x0b])
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x4020), (Ecx, 0x8877_6655)])
            .memory(0x4020, &[0x80], ReadOnly).fault(0x4020, 3),
        Case::preserving_flags("CMPXCHG match also requires write permission", &[0x66, 0x0f, 0xb1, 0x0b])
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x4020), (Ecx, 0x8877_6655)])
            .memory(0x4020, &[0x11, 0x22], ReadOnly).fault(0x4020, 3),
        Case::preserving_flags("CMPXCHG second-page failure preserves both destinations and flags", &[0x0f, 0xb1, 0x0b])
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x4ffe), (Ecx, 0x8877_6655)])
            .memory(0x4ffe, &[0x11, 0x22], ReadWrite).memory(0x5000, &[0x33, 0x44], ReadOnly)
            .fault(0x5000, 3),
    ]
}
test_cases!(write_faults_preserve_all_outputs, memory_faults());

#[rustfmt::skip]
fn scattered_memory() -> Vec<Case> {
    vec![
        Case::replacing_flags("cross-page XADD reads the old dword and writes its sum", &[0x0f, 0xc1, 0x03],
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 1, 0xffff_ffff).initial_register(Ebx, 0x4ffe)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[0x5a, 0xff, 0xff, 0xff, 0xff, 0x5a], ReadWrite)
            .expect_memory(0x4ffe, &[0, 0, 0, 0]),
        Case::replacing_flags("cross-page CMPXCHG retains its equal accumulator and stores old CX", &[0x66, 0x0f, 0xb1, 0x0b],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_ff80).initial_register(Ebx, 0x4fff).initial_register(Ecx, 0x8877_6655)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffe, &[0x5a, 0x80, 0xff, 0x5a], ReadWrite)
            .expect_memory(0x4fff, &[0x55, 0x66]),
    ]
}

test_cases!(scattered_writes_keep_canaries, scattered_memory());

#[rustfmt::skip]
fn exchanges_and_flags() -> Vec<Sequence> {
    vec![Sequence::new("completed exchanges and consumed flags precede a failed CMPXCHG write", Flags::all(true))
        .initial_register(Eax, 0x4433_01ff).initial_register(Ebx, 0x4000)
        .initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99)
        .initial_register(Ebp, 0x5000).initial_register(Esi, 0x7777_7ffe).initial_register(Edi, 0x4000)
        .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadOnly)
        .backing(0x7fff, &[0x5a, 0xff, 0xff, 0x5a])
        .backing(0x9fff, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a])
        .step(Step::new(&[0x0f, 0xc0, 0xe0],
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_ff00))
        .step(Step::preserving_flags(&[0x0f, 0x92, 0xc2]).register(Edx, 0xccbb_aa01))
        .step(Step::new(&[0x0f, 0xb0, 0xe3],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Ebx, 0x40ff))
        .step(Step::preserving_flags(&[0x0f, 0x94, 0xc6]).register(Edx, 0xccbb_0101))
        .step(Step::new(&[0x66, 0x0f, 0xc1, 0x17],
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Edx, 0xccbb_ffff).expect_memory(0x4000, &[0, 1]))
        .step(Step::new(&[0x66, 0x0f, 0xb1, 0xd6],
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_7ffe))
        .step(Step::preserving_flags(&[0x0f, 0xb1, 0x4d, 0]).fault(0x5000, 3))]
}

test_sequences!(
    exchange_results_flags_and_later_fault,
    exchanges_and_flags()
);

test_sequences!(
    signed_and_unsigned_comparison_consumers,
    [
        Sequence::from_opaque_flags("CMPXCHG mismatch feeds signed and unsigned conditions")
            .initial_registers(&[(Eax, 0x4433_227f), (Ecx, 0x8877_66ff), (Edx, 0xccbb_aa99)])
            .step(
                Step::new(
                    &[0x0f, 0xb0, 0xd1],
                    Flags {
                        cf: Set,
                        pf: Clear,
                        af: Clear,
                        zf: Clear,
                        sf: Set,
                        of: Set
                    }
                )
                .register(Eax, 0x4433_22ff)
            )
            .step(Step::preserving_flags(&[0x0f, 0x9c, 0xc2]).register(Edx, 0xccbb_aa00))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc6]).register(Edx, 0xccbb_0100))
    ]
);

#[test]
fn forms_consume_an_address_without_an_immediate() {
    for code in [
        &[0x0f, 0xc0, 0xe0][..],
        &[0x66, 0x0f, 0xc1, 0xd8],
        &[0x0f, 0xb0, 0xe3],
        &[0x66, 0x0f, 0xb1, 0x44, 0x8b, 0x80],
        &[0x0f, 0xc1, 0x04, 0x25, 0x20, 0x40, 0, 0],
    ] {
        check_length(code);
    }
}
