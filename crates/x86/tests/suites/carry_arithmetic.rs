use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set, Undefined},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};

#[rustfmt::skip]
fn arithmetic_boundaries() -> Vec<Case> {
    vec![
        Case::new("ADC dword with clear carry", &[0x11, 0xd8], Flags::all(false),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0000, 0x0000_0000).initial_register(Ebx, 0x0000_0000),
        Case::new("ADC byte accumulator immediate consumes carry", &[0x14, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2200, 0x4433_2201),
        Case::new("ADC byte group immediate carries out of the width", &[0x80, 0xd0, 0x00], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_22ff, 0x4433_2200),
        Case::new("ADC word equal maximum operands retain carry", &[0x66, 0x11, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_ffff, 0x4433_ffff).initial_register(Ebx, 0x0000_ffff),
        Case::new("ADC dword maximum immediate plus carry must not truncate first", &[0x15, 0xff, 0xff, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0000, 0x0000_0000),
        Case::new("ADC word carry crosses the positive signed limit", &[0x66, 0x15, 0x00, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7fff, 0x4433_8000),
        Case::new("ADC dword right operand plus carry changes sign", &[0x13, 0xc3], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x0000_0000, 0x8000_0000).initial_register(Ebx, 0x7fff_ffff),
        Case::new("ADC dword mixed signs carry without signed overflow", &[0x11, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x8000_0000, 0x0000_0000).initial_register(Ebx, 0x7fff_ffff),
        Case::new("ADC dword negative self addition overflows with clear carry in", &[0x11, 0xc0], Flags::all(false),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x0000_0000),
        Case::new("ADC byte reverse form carries across a nibble", &[0x12, 0xc3], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_220f, 0x4433_2210).initial_register(Ebx, 0x0000_0000),
        Case::new("SBB word with clear borrow", &[0x66, 0x19, 0xd8], Flags::all(false),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0000, 0x4433_0000).initial_register(Ebx, 0x0000_0000),
        Case::new("SBB byte clear borrow still subtracts its source", &[0x18, 0xd8], Flags::all(false),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2200, 0x4433_22ff).initial_register(Ebx, 1),
        Case::new("SBB byte accumulator immediate borrows from zero", &[0x1c, 0x00], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2200, 0x4433_22ff),
        Case::new("SBB word borrow can exactly consume the left operand", &[0x66, 0x1d, 0x00, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0000),
        Case::new("SBB word maximum immediate plus borrow must not truncate first", &[0x66, 0x81, 0xd8, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0000, 0x4433_0000),
        Case::new("SBB dword equal maximum operands retain borrow", &[0x19, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xffff_ffff, 0xffff_ffff).initial_register(Ebx, 0xffff_ffff),
        Case::new("SBB word borrow crosses the negative signed limit", &[0x66, 0x1b, 0xc3], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_8000, 0x4433_7fff).initial_register(Ebx, 0x0000_0000),
        Case::new("SBB byte right operand plus borrow changes sign without overflow", &[0x1a, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2200, 0x4433_2280).initial_register(Ebx, 0x0000_007f),
        Case::new("SBB dword mixed signs overflow to zero without borrow out", &[0x1d, 0xff, 0xff, 0xff, 0x7f], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x0000_0000),
        Case::new("SBB dword mixed signs produce both borrow and overflow", &[0x19, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x7fff_ffff, 0xffff_fffe).initial_register(Ebx, 0x8000_0000),
        Case::new("SBB byte group immediate borrows across a nibble", &[0x80, 0xd8, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2210, 0x4433_220f),
    ]
}

#[rustfmt::skip]
fn immediates_and_aliases() -> Vec<Case> {
    vec![
        Case::new("ADC AX, full group immediate -1", &[0x66, 0x81, 0xd0, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001),
        Case::new("ADC AX, sign-extended immediate 7f", &[0x66, 0x83, 0xd0, 0x7f], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0081),
        Case::new("ADC EAX, sign-extended immediate 80", &[0x83, 0xd0, 0x80], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x0000_0001, 0xffff_ff82),
        Case::new("ADC AL,AH reads old byte aliases", &[0x10, 0xe0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_7f80, 0x4433_7f00),
        Case::new("ADC AH,AH reads old byte aliases", &[0x10, 0xe4], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7f80, 0x4433_ff80),
        Case::new("SBB AX, sign-extended immediate 7f", &[0x66, 0x83, 0xd8, 0x7f], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_ff81),
        Case::new("SBB EAX, full group immediate -1", &[0x81, 0xd8, 0xff, 0xff, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001),
        Case::new("SBB EAX, sign-extended immediate 80", &[0x83, 0xd8, 0x80], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0080),
        Case::new("SBB AH,AL reads old byte aliases", &[0x18, 0xc4], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7f80, 0x4433_fe80),
        Case::new("SBB EAX,EAX reads its old self operand", &[0x19, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_8000, 0xffff_ffff),
    ]
}

#[rustfmt::skip]
fn memory_operands() -> Vec<Case> {
    vec![
        Case::new("ADC byte destination uses its old address and AL", &[0x10, 0x00], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4020).memory(0x401f, &[0xa5, 0xdf, 0xa5], ReadWrite)
            .expect_memory(0x4020, &[0]),
        Case::new("SBB dword source uses the old destination as its address", &[0x1b, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4020, 0x401a).memory(0x4020, &[5, 0, 0, 0], ReadOnly),
        Case::new("ADC word reads a source across scattered pages", &[0x66, 0x13, 0x03], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_ffff, 0x4433_0000).initial_register(Ebx, 0x4fff)
            .map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly).memory(0x4fff, &[0, 0], ReadOnly),
        Case::new("SBB word writes a destination across scattered pages", &[0x66, 0x19, 0x03], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .initial_registers(&[(Eax, 0x4433_0000), (Ebx, 0x4fff)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffe, &[0xa5, 0xff, 0xff, 0x5a], ReadWrite).expect_memory(0x4fff, &[0xfe, 0xff]),
        Case::new("ADC dword memory destination takes a signed byte immediate", &[0x83, 0x53, 0x80, 0x80], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .initial_register(Ebx, 0x40a0).memory(0x4020, &[1, 0, 0, 0], ReadWrite)
            .expect_memory(0x4020, &[0x82, 0xff, 0xff, 0xff]),
        Case::new("SBB byte immediate can leave memory unchanged while replacing flags", &[0x80, 0x1b, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .initial_register(Ebx, 0x4020).memory(0x4020, &[1], ReadWrite).expect_memory(0x4020, &[1]),
        Case::new("ADC dword memory destination replaces flags after addition", &[0x11, 0x03], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .initial_registers(&[(Eax, 0), (Ebx, 0x4020)]).memory(0x4020, &[0xff, 0xff, 0xff, 0x7f], ReadWrite)
            .expect_memory(0x4020, &[0, 0, 0, 0x80]),
        Case::new("SBB byte reads only the final mapped source byte", &[0x1a, 0x03], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_00ff, 0x4433_00fe).initial_register(Ebx, 0x4fff).memory(0x4fff, &[0], ReadOnly),
    ]
}

#[rustfmt::skip]
fn access_faults() -> Vec<Case> {
    vec![
        Case::new("ADC missing byte source preserves entry state", &[0x12, 0x03], Flags::all(true), Flags::all(Preserved))
            .preserve_flag_record().initial_register(Ebx, 0x5000).fault(0x5000, 0),
        Case::new("SBB incomplete word source preserves entry state", &[0x66, 0x1b, 0x03], Flags::all(true), Flags::all(Preserved))
            .preserve_flag_record().initial_register(Ebx, 0x4fff).memory(0x4fff, &[0xff], ReadOnly).fault(0x5000, 0),
        Case::new("SBB read-only byte destination faults even when arithmetic would leave it unchanged", &[0x80, 0x1b, 0xff], Flags::all(true), Flags::all(Preserved))
            .preserve_flag_record().initial_register(Ebx, 0x4020).memory(0x4020, &[1], ReadOnly).fault(0x4020, 3),
        Case::new("ADC missing second destination page prevents partial writes and flags", &[0x11, 0x03], Flags::all(true), Flags::all(Preserved))
            .preserve_flag_record().initial_register(Ebx, 0x4fff).memory(0x4ffe, &[0xa5, 0xff], ReadWrite).fault(0x5000, 2),
        Case::new("SBB read-only second destination page prevents partial writes and flags", &[0x19, 0x03], Flags::all(true), Flags::all(Preserved))
            .preserve_flag_record().initial_register(Ebx, 0x4fff)
            .memory(0x4ffe, &[0xa5, 0xff], ReadWrite).memory(0x5000, &[0xff, 0xff, 0xff, 0x5a], ReadOnly).fault(0x5000, 3),
    ]
}

#[rustfmt::skip]
fn carry_sequences() -> Vec<Sequence> {
    vec![
        Sequence::new("ADC consumes carry from ADD instead of stale clear CF", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_22ff), (Ebx, 1), (Ecx, 0)])
            .step(Step::new(&[0x00, 0xd8],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x4433_2200))
            .step(Step::new(&[0x83, 0xd1, 0], Flags::all(Clear)).register(Ecx, 1)),
        Sequence::new("SBB consumes borrow from SUB instead of stale clear CF", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_0000), (Ebx, 1), (Ecx, 0)])
            .step(Step::new(&[0x66, 0x29, 0xd8],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_ffff))
            .step(Step::new(&[0x83, 0xd9, 0],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Ecx, 0xffff_ffff)),
        Sequence::new("ADC consumes clear carry from XOR instead of stale set CF", Flags::all(true))
            .initial_registers(&[(Eax, 0xffff_ffff), (Ecx, 0x4433_0000)])
            .step(Step::new(&[0x31, 0xc0],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .step(Step::new(&[0x66, 0x83, 0xd1, 0],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })),
        Sequence::new("mixed-width carry chain replaces cached incoming CF", Flags::all(true))
            .initial_registers(&[(Eax, 0xffff_ffff), (Ebx, 0)])
            .step(Step::new(&[0x10, 0xd8],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0xffff_ff00))
            .step(Step::new(&[0x66, 0x19, 0xd8],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0xffff_feff))
            .step(Step::new(&[0x11, 0xd8],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })),
        Sequence::new("failed SBB destination publishes completed ADC", Flags::all(true))
            .initial_registers(&[(Ecx, 0xffff_ffff), (Edx, 1), (Ebx, 0x4fff)])
            .memory(0x4ffe, &[0xa5, 0xff], ReadWrite)
            .step(Step::new(&[0x11, 0xd1],
                Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Ecx, 1))
            .step(Step::preserving_flags(&[0x19, 0x03]).fault(0x5000, 2))
            .trailing_code(&[0xb9, 0, 0, 0, 0], 1),
    ]
}

test_cases!(carry_borrow_and_signed_boundaries, arithmetic_boundaries());
test_cases!(
    immediates_and_old_register_aliases,
    immediates_and_aliases()
);
test_cases!(memory_sources_and_destinations, memory_operands());
test_cases!(access_faults_preserve_entry_state, access_faults());
test_sequences!(pending_carry_and_fault_publication, carry_sequences());
