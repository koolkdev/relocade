use super::code16;
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Ecx, Edi, Esi},
    Segment, StoredSegment,
};

fn record(backward: bool) -> wasm86_x86::StoredFlags {
    let mut flags = CpuState::filled(0xa5).flags;
    flags.bytes.df = u8::from(backward);
    flags
}

fn repetition() -> Vec<Case> {
    let mut cases = Vec::new();
    for default16 in [false, true] {
        for backward in [false, true] {
            for (opcode, width) in [(0xa4, 1u32), (0xa5, 2), (0xaa, 1), (0xab, 2)] {
                let mut code = if default16 { vec![] } else { vec![0x66, 0x67] };
                code.extend([0xf3, opcode]);
                let (start, next) = if backward {
                    (width, 0x10000 - width)
                } else {
                    (0x10000 - width, width)
                };
                let source = &[0x12, 0x34][..width as usize];
                let mut case = Case::preserving_flags(format!("16-bit REP indices and count CS.D16={default16} opcode={opcode:02x} DF={backward}"), &code)
                    .stored_flags(record(backward)).initial_register(Eax, 0x3412)
                    .register(Ecx, 0xaaaa_0002, 0xaaaa_0000)
                    .register(Edi, 0xbbbb_0000 | start, 0xbbbb_0000 | next)
                    .memory(start, &vec![0xff; width as usize], ReadWrite)
                    .memory(0, &vec![0xff; width as usize], ReadWrite)
                    .expect_memory(start, source).expect_memory(0, source);
                if opcode == 0xa4 || opcode == 0xa5 {
                    case = case
                        .register(
                            Esi,
                            0xcccc_4000,
                            if backward {
                                0xcccc_4000 - 2 * width
                            } else {
                                0xcccc_4000 + 2 * width
                            },
                        )
                        .memory(
                            if backward { 0x4000 - width } else { 0x4000 },
                            &source.repeat(2),
                            ReadOnly,
                        );
                }
                cases.push(if default16 { code16(case) } else { case });
            }
        }
        let prefix = if default16 { vec![] } else { vec![0x67] };
        for opcode in [0xa4, 0xaa] {
            let code = [&prefix[..], &[0xf3, opcode]].concat();
            let case =
                Case::preserving_flags("CX zero skips all operands despite upper ECX", &code)
                    .segmented_only()
                    .stored_flags(record(false))
                    .initial_register(Ecx, 0xaaaa_0000)
                    .segment(Segment::Ds, StoredSegment::unusable(0))
                    .segment(Segment::Es, StoredSegment::unusable(0));
            cases.push(if default16 { code16(case) } else { case });
        }
    }
    cases.push(
        Case::preserving_flags(
            "REP fault retains 16-bit aliases after a completed element",
            &[0x67, 0xf3, 0xa4],
        )
        .stored_flags(record(false))
        .register(Ecx, 0xaaaa_0003, 0xaaaa_0002)
        .register(Esi, 0xbbbb_4fff, 0xbbbb_5000)
        .register(Edi, 0xcccc_7000, 0xcccc_7001)
        .memory(0x4fff, &[0x12], ReadOnly)
        .memory(0x7000, &[0xff; 3], ReadWrite)
        .expect_memory(0x7000, &[0x12, 0xff, 0xff])
        .fault(0x5000, 0),
    );
    cases.push(code16(
        Case::preserving_flags(
            "67 uses full ECX and indices in 16-bit code",
            &[0x67, 0xf3, 0xa4],
        )
        .stored_flags(record(false))
        .register(Ecx, 0x10000, 0xffff)
        .register(Esi, 0x14fff, 0x15000)
        .register(Edi, 0x17000, 0x17001)
        .memory(0x14fff, &[0x12], ReadOnly)
        .memory(0x17000, &[0xff; 2], ReadWrite)
        .expect_memory(0x17000, &[0x12, 0xff])
        .fault(0x15000, 0),
    ));
    cases
}

test_cases!(
    rep_uses_address_sized_registers_and_precise_partial_progress,
    repetition()
);

fn single_elements() -> Vec<Case> {
    use crate::support::cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    };
    let equal = Flags {
        cf: Clear,
        pf: Set,
        af: Clear,
        zf: Set,
        sf: Clear,
        of: Clear,
    };
    vec![
        Case::preserving_flags("LODS with 16-bit index preserves upper ESI", &[0x67, 0xad])
            .stored_flags(record(false))
            .register(Esi, 0xabcd_fffc, 0xabcd_0000)
            .register(Eax, 0, 0x1234_5678)
            .memory(0xfffc, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::replacing_flags("CMPS wraps both 16-bit indices", &[0x67, 0xa6], equal)
            .stored_flags(record(false))
            .register(Esi, 0xabcd_ffff, 0xabcd_0000)
            .register(Edi, 0x1234_ffff, 0x1234_0000)
            .memory(0xffff, &[0x78], ReadOnly),
        Case::replacing_flags(
            "SCAS wraps DI backward and preserves upper EDI",
            &[0x67, 0xaf],
            equal,
        )
        .stored_flags(record(true))
        .register(Edi, 0xabcd_0000, 0xabcd_fffc)
        .initial_register(Eax, 0x1234_5678)
        .memory(0, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
    ]
}

test_cases!(
    single_string_elements_use_address_sized_indices,
    single_elements()
);
