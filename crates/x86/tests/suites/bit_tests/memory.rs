//! Memory bit indexes select a signed offset in operand-sized units.
use super::{bit_flags, INITIAL_FLAGS};
use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::*;

fn signed_indexes() -> Vec<Case> {
    let mut cases = Vec::new();
    // Literal addresses cover floor division at negative boundaries and width truncation.
    for (word, base, index, address, carry) in [
        (true, 0x4004, 0xffff, 0x4002, Set),
        (true, 0x4004, 0xfff0, 0x4002, Set),
        (true, 0x4004, 0xffef, 0x4000, Set),
        (false, 0x4008, 0xffff_ffff, 0x4004, Set),
        (false, 0x4008, 0xffff_ffe0, 0x4004, Set),
        (false, 0x4008, 0xffff_ffdf, 0x4000, Set),
        (true, 0x4000, 16, 0x4002, Set),
        (true, 0x4000, 17, 0x4002, Clear),
        (false, 0x4000, 32, 0x4004, Set),
        (false, 0x4000, 33, 0x4004, Clear),
        (true, 0x5000, 0x1234_8000, 0x4000, Set),
        (true, 0x4000, 0xabcd_7fff, 0x4ffe, Set),
        (false, 0x1000_4000, 0x8000_0000, 0x4000, Set),
        (false, 0xf000_4004, 0x7fff_ffff, 0x4000, Set),
        (true, 0x4003, 16, 0x4005, Set),
        (true, 0x5001, 0xffff, 0x4fff, Set),
        (false, 0x5002, 0xffff_ffff, 0x4ffe, Set),
        (true, 0xffff_fffe, 16, 0, Set),
        (false, 0, 0xffff_ffff, 0xffff_fffc, Set),
    ] {
        let (code, bytes) = if word {
            (&[0x66, 0x0f, 0xa3, 0x13][..], &[1, 0x80][..])
        } else {
            (&[0x0f, 0xa3, 0x13][..], &[1, 0, 0, 0x80][..])
        };
        cases.push(
            Case::new(
                format!("BT word={word}, base={base:x}, index={index:x}"),
                code,
                INITIAL_FLAGS,
                bit_flags(carry),
            )
            .initial_registers(&[(Ebx, base), (Edx, index)])
            .memory(address, bytes, ReadOnly),
        );
    }
    cases
}
test_cases!(
    signed_register_indexes_choose_the_operand_address,
    signed_indexes()
);

#[rustfmt::skip]
fn immediate_indexes() -> Vec<Case> {
    vec![
        Case::new("BTS word masks immediate 255 without adjusting the base", &[0x66, 0x0f, 0xba, 0x2b, 255],
            INITIAL_FLAGS, bit_flags(Set)).initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[1, 0x80], ReadWrite),
        Case::new("BTR dword masks immediate 255 to bit 31", &[0x0f, 0xba, 0x33, 255],
            INITIAL_FLAGS, bit_flags(Set)).initial_register(Ebx, 0x4ffc)
            .memory(0x4ffc, &[1, 0, 0, 0x80], ReadWrite).expect_memory(0x4ffc, &[1, 0, 0, 0]),
        Case::new("BTC word masks immediate 16 to bit zero", &[0x66, 0x0f, 0xba, 0x3b, 16],
            INITIAL_FLAGS, bit_flags(Set)).initial_register(Ebx, 0x4ffe)
            .memory(0x4ffe, &[1, 0x80], ReadWrite).expect_memory(0x4ffe, &[0, 0x80]),
        Case::new("BT dword masks immediate 32 without adjusting the base", &[0x0f, 0xba, 0x23, 32],
            INITIAL_FLAGS, bit_flags(Set)).initial_register(Ebx, 0x4ffc).memory(0x4ffc, &[1, 0, 0, 0x80], ReadOnly),
    ]
}
test_cases!(
    immediates_never_change_the_operand_address,
    immediate_indexes()
);

#[rustfmt::skip]
fn aliased_addresses() -> Vec<Case> {
    vec![
        Case::new("BT uses EBX as both base and signed index", &[0x0f, 0xa3, 0x1b], INITIAL_FLAGS, bit_flags(Set))
            .initial_register(Ebx, 0x4000).memory(0x4800, &[0xa5, 0x5a, 0x5a, 0xa5], ReadOnly),
        Case::new("BTS uses full ECX as base but only CX as index", &[0x66, 0x0f, 0xab, 0x09], INITIAL_FLAGS, bit_flags(Clear))
            .initial_register(Ecx, 0xffff_4001).memory(0xffff_4801, &[0xa5, 0x5a], ReadWrite)
            .expect_memory(0xffff_4801, &[0xa7, 0x5a]),
        Case::new("BTR uses EBP as address index and negative bit index", &[0x0f, 0xb3, 0x2c, 0x2b], INITIAL_FLAGS, bit_flags(Set))
            .initial_registers(&[(Ebx, 0x4005), (Ebp, u32::MAX)])
            .memory(0x4000, &[0xa5, 0x5a, 0x5a, 0xa5], ReadWrite).expect_memory(0x4000, &[0xa5, 0x5a, 0x5a, 0x25]),
        Case::new("BTC scales ECX for the address but not the bit index", &[0x0f, 0xbb, 0x4c, 0x8b, 0xfc], INITIAL_FLAGS, bit_flags(Clear))
            .initial_registers(&[(Ebx, 0x4010), (Ecx, 0x4000_0001)])
            .memory(0x0800_4010, &[0xa5, 0x5a, 0x5a, 0xa5], ReadWrite).expect_memory(0x0800_4010, &[0xa7, 0x5a, 0x5a, 0xa5]),
        Case::new("BTR negative index writes the complete word across scattered pages", &[0x66, 0x0f, 0xb3, 0x13],
            INITIAL_FLAGS, bit_flags(Set)).initial_registers(&[(Ebx, 0x5001), (Edx, 0xffff)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffe, &[0x5a, 1, 0x80, 0x5a], ReadWrite).expect_memory(0x4fff, &[1, 0]),
    ]
}
test_cases!(
    address_dependencies_and_scattered_writes,
    aliased_addresses()
);

#[rustfmt::skip]
fn faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("BTS already-set bit still needs write permission", &[0x0f, 0xba, 0x2b, 0])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[1, 0, 0, 0], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("BTR already-clear bit still needs write permission", &[0x66, 0x0f, 0xba, 0x33, 0])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0, 0], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("BTC requires write permission", &[0x0f, 0xba, 0x3b, 31])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0, 0, 0, 0], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("BT faults at the adjusted address despite a readable encoded base", &[0x66, 0x0f, 0xa3, 0x13])
            .initial_registers(&[(Ebx, 0x5000), (Edx, 0x1234_8000)])
            .memory(0x5000, &[1, 0x80], ReadOnly).fault(0x4000, 0),
        Case::preserving_flags("BT reads the complete word even when bit zero is readable", &[0x66, 0x0f, 0xba, 0x23, 0])
            .initial_register(Ebx, 0x4fff).memory(0x4fff, &[1], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("BTR negative index cannot partially write before a missing page", &[0x0f, 0xb3, 0x13])
            .initial_registers(&[(Ebx, 0x5002), (Edx, u32::MAX)])
            .memory(0x4ffe, &[1, 0], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("BTS rejects the read-only first page before the absent second page", &[0x66, 0x0f, 0xba, 0x2b, 0])
            .initial_register(Ebx, 0x4fff).memory(0x4fff, &[1], ReadOnly).fault(0x4fff, 3),
        Case::preserving_flags("BT high immediate still requires the complete word", &[0x66, 0x0f, 0xba, 0x23, 255])
            .initial_register(Ebx, 0x4fff).memory(0x4fff, &[1], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("BT adjusted word read wraps at the linear address boundary", &[0x66, 0x0f, 0xa3, 0x13])
            .initial_registers(&[(Ebx, 1), (Edx, 0xffff)])
            .memory(u32::MAX, &[1], ReadOnly).fault(0, 0),
    ]
}
test_cases!(access_width_write_intent_and_fault_atomicity, faults());
