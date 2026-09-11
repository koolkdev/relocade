use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

#[rustfmt::skip]
fn earlier_arithmetic_before_faults() -> Vec<SequenceCase> {
    let mut cases = Vec::new();
    for (name, code, first, second, address, error) in [
        ("readonly RMW preserves earlier flags", &[0x01, 0x03][..], ReadOnly, None, 0x4ffe, 3),
        ("missing RMW tail keeps every byte", &[0x01, 0x03][..], ReadWrite, None, 0x5000, 2),
        ("readonly RMW tail keeps every byte", &[0x01, 0x03][..], ReadWrite, Some(ReadOnly), 0x5000, 3),
        ("ADD source fault preserves earlier flags and destination", &[0x03, 0x03][..], ReadOnly, None, 0x5000, 0),
        ("SUB denied destination preserves earlier flags", &[0x29, 0x03][..], ReadOnly, None, 0x4ffe, 3),
        ("AND missing tail prevents even a constant-zero store", &[0x81, 0x23, 0, 0, 0, 0][..], ReadWrite, None, 0x5000, 2),
        ("OR readonly tail keeps every byte", &[0x09, 0x03][..], ReadWrite, Some(ReadOnly), 0x5000, 3),
        ("XOR source fault preserves earlier flags and destination", &[0x33, 0x03][..], ReadOnly, None, 0x5000, 0),
        ("TEST missing tail is a read fault", &[0x85, 0x03][..], ReadOnly, None, 0x5000, 0),
        ("TEST zero immediate still checks the complete read span", &[0xf7, 0x03, 0, 0, 0, 0][..], ReadOnly, None, 0x5000, 0),
        ("CMP missing tail is a read fault", &[0x39, 0x03][..], ReadWrite, None, 0x5000, 0),
    ] {
        let mut case = SequenceCase::new(name, Flags::all(true))
            .initial_register(Eax, 1).initial_register(Ebx, 0x4ffe)
            .initial_register(Ecx, 0x7fff_fffe).initial_register(Edx, 2)
            .map_page(4, 0x8000, first)
            .backing(0x8ffd, &[0xa5, 0xff, 0xff]).backing(0xa000, &[0xff, 0xff, 0x5a])
            .step(Checkpoint::new(&[0x01, 0xd1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Ecx, 0x8000_0000))
            .step(Checkpoint::preserving_flags(code).fault(address, error));
        if let Some(permissions) = second { case = case.map_page(5, 0xa000, permissions); }
        cases.push(case);
    }
    cases
}

fn denied_condition_store() -> Vec<Case> {
    vec![Case::new(
        "SETcc denied destination preserves incoming flags",
        &[0x0f, 0x94, 0x03],
        Flags::all(true),
        Flags::all(Preserved),
    )
    .preserve_flag_record()
    .initial_register(Ebx, 0x4000)
    .memory(0x4000, &[0xa5], ReadOnly)
    .fault(0x4000, 3)]
}

test_sequences!(
    operand_faults_preserve_prior_progress,
    earlier_arithmetic_before_faults()
);
test_cases!(setcc_write_denial, denied_condition_store());
