//! Single-element strings, repeated transfers and restartable comparisons.

use crate::flags::Flag;
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError, CpuState, Gpr32::*, StoredFlags, StoredStatusSource,
};

#[path = "strings/conditional_repetition.rs"]
mod conditional_repetition;
#[path = "strings/repeated_loads.rs"]
mod repeated_loads;
#[path = "strings/repetition.rs"]
mod repetition;
#[path = "strings/restart.rs"]
mod restart;

// Fixture masks use CF/PF/AF/ZF/SF/OF; expected masks are literal status-flag results.
fn flags(bits: u8) -> Flags<FlagExpectation> {
    let bit = |mask| if bits & mask != 0 { Set } else { Clear };
    Flags {
        cf: bit(1),
        pf: bit(2),
        af: bit(4),
        zf: bit(8),
        sf: bit(16),
        of: bit(32),
    }
}

fn record(df: u8) -> StoredFlags {
    let mut record = CpuState::filled(0xa5).flags;
    record.status_source = StoredStatusSource {
        kind: 0,
        reserved: [0x5a, 0xc3, 0x96],
        left: 0x1234_5678,
        right: 0x8765_4321,
    };
    record.bytes.cf = 0;
    record.bytes.pf = 0;
    record.bytes.af = 0;
    record.bytes.zf = 1;
    record.bytes.sf = 0;
    record.bytes.of = 0;
    record.bytes.df = df;
    record.bytes.tf = 0x80;
    record.bytes.nt = 0xff;
    record.bytes.ac = 0xfe;
    record.bytes.id = 0x81;
    record
}

#[rustfmt::skip]
fn transfers() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVSB executes once even with zero ECX", &[0xa4]).stored_flags(record(0))
            .initial_register(Ecx, 0).register(Esi, 0x4000, 0x4001).register(Edi, 0x6000, 0x6001)
            .memory(0x4000, &[0x12], ReadOnly).memory(0x6000, &[0xa5], ReadWrite).expect_memory(0x6000, &[0x12]),
        Case::preserving_flags("MOVSW decrements both indices after copying one word", &[0x66, 0xa5]).stored_flags(record(1))
            .register(Esi, 0x4002, 0x4000).register(Edi, 0x6002, 0x6000)
            .memory(0x4002, &[0x12, 0x34], ReadOnly).memory(0x6001, &[0xa5; 4], ReadWrite)
            .expect_memory(0x6002, &[0x12, 0x34]),
        Case::preserving_flags("STOSB stores AL and advances only EDI", &[0xaa]).stored_flags(record(0))
            .initial_register(Eax, 0xa1b2_c3d4).register(Edi, 0x6000, 0x6001)
            .memory(0x6000, &[0xa5; 2], ReadWrite).expect_memory(0x6000, &[0xd4]),
        Case::preserving_flags("STOSW stores AX and decrements only EDI", &[0x66, 0xab]).stored_flags(record(1))
            .initial_register(Eax, 0xa1b2_c3d4).register(Edi, 0x6002, 0x6000)
            .memory(0x6001, &[0xa5; 4], ReadWrite).expect_memory(0x6002, &[0xd4, 0xc3]),
        Case::preserving_flags("STOSD stores full EAX and advances only EDI", &[0xab]).stored_flags(record(0))
            .initial_register(Eax, 0xa1b2_c3d4).register(Edi, 0x6000, 0x6004)
            .memory(0x6000, &[0xa5; 4], ReadWrite).expect_memory(0x6000, &[0xd4, 0xc3, 0xb2, 0xa1]),
        Case::preserving_flags("LODSB reads only the final mapped byte and preserves upper EAX", &[0xac]).stored_flags(record(0))
            .initial_register(Ecx, 0).register(Eax, 0xa1b2_c3d4, 0xa1b2_c312).register(Esi, 0x4fff, 0x5000)
            .memory(0x4fff, &[0x12], ReadOnly),
        Case::preserving_flags("LODSW preserves upper EAX and decrements ESI", &[0x66, 0xad]).stored_flags(record(1))
            .register(Eax, 0xa1b2_c3d4, 0xa1b2_3412).register(Esi, 0x4002, 0x4000)
            .memory(0x4002, &[0x12, 0x34], ReadOnly),
        Case::preserving_flags("LODSD replaces full EAX and advances ESI", &[0xad]).stored_flags(record(0))
            .register(Eax, 0xa1b2_c3d4, 0x7856_3412).register(Esi, 0x4000, 0x4004)
            .memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadOnly),
        Case::preserving_flags("MOVSD captures an overlapping source across scattered pages", &[0xa5])
            .stored_flags(record(0xfe)).register(Esi, 0x4ffe, 0x5002).register(Edi, 0x4fff, 0x5003)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[1, 2, 3, 4, 5, 6, 7], ReadWrite).expect_memory(0x4ffd, &[1, 2, 2, 3, 4, 5, 7]),
        Case::preserving_flags("MOVSD physical alias uses source bytes before the overlapping write", &[0xa5])
            .stored_flags(record(0xff)).register(Esi, 0x4000, 0x3ffc).register(Edi, 0x7001, 0x6ffd)
            .map_page(4, 0x8000, ReadOnly).map_page(7, 0x8000, ReadWrite).backing(0x8000, &[1, 2, 3, 4, 5, 6])
            .expect_memory(0x7001, &[1, 2, 3, 4]),
    ]
}

#[rustfmt::skip]
fn comparisons() -> Vec<Case> {
    vec![
        Case::replacing_flags("CMPSB subtracts source minus destination before decrementing both indices", &[0xa6], flags(23))
            .stored_flags(record(1)).initial_register(Ecx, 0).register(Esi, 0x4001, 0x4000).register(Edi, 0x6001, 0x6000)
            .memory(0x4001, &[0], ReadOnly).memory(0x6001, &[1], ReadOnly),
        Case::replacing_flags("CMPSW compares complete words and reports signed overflow", &[0x66, 0xa7], flags(38))
            .stored_flags(record(0)).register(Esi, 0x4000, 0x4002).register(Edi, 0x6000, 0x6002)
            .memory(0x4000, &[0, 0x80], ReadOnly).memory(0x6000, &[1, 0], ReadOnly),
        Case::replacing_flags("CMPSD reports equality without changing either input", &[0xa7], flags(10))
            .stored_flags(record(0)).register(Esi, 0x4000, 0x4004).register(Edi, 0x6000, 0x6004)
            .memory(0x4000, &[1, 2, 3, 4], ReadOnly).memory(0x6000, &[1, 2, 3, 4], ReadOnly),
        Case::replacing_flags("SCASB subtracts from AL and ignores disagreeing upper EAX", &[0xae], flags(49))
            .stored_flags(record(0)).initial_register(Eax, 0xabcd_007f).register(Edi, 0x6000, 0x6001)
            .memory(0x6000, &[0xff], ReadOnly),
        Case::replacing_flags("SCASW subtracts from AX and decrements only EDI", &[0x66, 0xaf], flags(6))
            .stored_flags(record(1)).initial_register(Eax, 0xabcd_0010).register(Edi, 0x6002, 0x6000)
            .memory(0x6002, &[1, 0], ReadOnly),
        Case::replacing_flags("SCASD uses the full accumulator", &[0xaf], flags(23))
            .stored_flags(record(0)).initial_register(Eax, 0x0100_0000).register(Edi, 0x6000, 0x6004)
            .memory(0x6000, &[1, 0, 0, 1], ReadOnly),
    ]
}

#[rustfmt::skip]
fn element_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVSB source fault precedes its missing destination", &[0xa4]).stored_flags(record(0))
            .initial_registers(&[(Esi, 0x4000), (Edi, 0x6000)]).fault(0x4000, 0),
        Case::preserving_flags("CMPSB source fault precedes its missing destination", &[0xa6]).stored_flags(record(1))
            .initial_registers(&[(Esi, 0x4000), (Edi, 0x6000)]).fault(0x4000, 0),
        Case::preserving_flags("LODSW incomplete source preserves EAX and ESI", &[0x66, 0xad]).stored_flags(record(0))
            .initial_registers(&[(Esi, 0x4fff), (Eax, 0xaabb_ccdd)])
            .memory(0x4fff, &[0x12], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("STOSD read-only destination preserves EDI and all bytes", &[0xab]).stored_flags(record(1))
            .initial_register(Edi, 0x6000).memory(0x6000, &[0xa5; 4], ReadOnly).fault(0x6000, 3),
        Case::preserving_flags("MOVSW incomplete destination preserves both indices and all bytes", &[0x66, 0xa5])
            .stored_flags(record(0)).initial_registers(&[(Esi, 0x4000), (Edi, 0x7fff)])
            .memory(0x4000, &[0x12, 0x34], ReadOnly).memory(0x7fff, &[0xa5], ReadWrite).fault(0x8000, 2),
        Case::preserving_flags("CMPSD incomplete destination cannot publish flags or either index", &[0xa7])
            .stored_flags(record(0)).initial_registers(&[(Esi, 0x4000), (Edi, 0x7fff)])
            .memory(0x4000, &[1, 2, 3, 4], ReadOnly).memory(0x7fff, &[1], ReadOnly).fault(0x8000, 0),
        Case::preserving_flags("SCASW missing operand preserves flags and EDI", &[0x66, 0xaf]).stored_flags(record(1))
            .initial_registers(&[(Eax, 0x1234_0000), (Edi, 0x6000)]).fault(0x6000, 0),
    ]
}

fn histories() -> Vec<Sequence> {
    let mut cases = Vec::new();
    for (df_opcode, df, source, destination, next_source, next_destination) in [
        (0xfc, false, 0x4000, 0x6000, 0x4001, 0x6001),
        (0xfd, true, 0x4001, 0x6001, 0x4000, 0x6000),
    ] {
        cases.push(
            Sequence::from_opaque_flags(format!(
                "direction control feeds byte load/store and preserves pending ADD, DF {df}"
            ))
            .stored_flags(record(if df { 0xfe } else { 0xff }))
            .initial_registers(&[
                (Eax, 0x4433_227f),
                (Esi, source),
                (Edi, destination),
                (Ecx, 0),
            ])
            .memory(0x4000, &[0x12, 0x12], ReadOnly)
            .memory(0x6000, &[0xa5; 2], ReadWrite)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x4433_2280))
            .step(Step::preserving_flags(&[df_opcode]).expect_direct_flag(Flag::DF, df))
            .step(
                Step::preserving_flags(&[0xac])
                    .register(Esi, next_source)
                    .register(Eax, 0x4433_2212),
            )
            .step(
                Step::preserving_flags(&[0xaa])
                    .register(Edi, next_destination)
                    .expect_memory(destination, &[0x12]),
            )
            .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9212)),
        );
    }
    cases.extend([
        Sequence::from_opaque_flags(
            "SCAS flags feed signed and unsigned conditions and carry arithmetic",
        )
        .stored_flags(record(0))
        .initial_registers(&[
            (Eax, 0x4433_2200),
            (Ebx, 0),
            (Ecx, 0xaabb_ccdd),
            (Edi, 0x6000),
        ])
        .memory(0x6000, &[1], ReadOnly)
        .step(Step::new(&[0xae], flags(23)).register(Edi, 0x6001))
        .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 0xaabb_cc01))
        .step(Step::preserving_flags(&[0x0f, 0x9c, 0xc5]).register(Ecx, 0xaabb_0101))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9700))
        .step(Step::new(&[0x83, 0xd3, 0], flags(0)).register(Ebx, 1)),
        Sequence::from_opaque_flags("faulting CMPS keeps the flags produced earlier in the block")
            .stored_flags(record(0))
            .initial_registers(&[(Eax, 0x7f), (Esi, 0x4000), (Edi, 0x6000)])
            .memory(0x4000, &[0], ReadOnly)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x80))
            .step(Step::preserving_flags(&[0xa6]).fault(0x6000, 0))
            .trailing_code(&[0xfc, 0xa4], 2),
        Sequence::from_opaque_flags("MOVSD preserves pending ADD until LAHF observes it")
            .stored_flags(record(0xfe))
            .initial_registers(&[(Eax, 0x4433_227f), (Esi, 0x4000), (Edi, 0x6000)])
            .memory(0x4000, &[1, 2, 3, 4], ReadOnly)
            .memory(0x6000, &[0xa5; 4], ReadWrite)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x4433_2280))
            .step(
                Step::preserving_flags(&[0xa5])
                    .register(Esi, 0x4004)
                    .register(Edi, 0x6004)
                    .expect_memory(0x6000, &[1, 2, 3, 4]),
            )
            .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9280)),
    ]);
    cases
}

#[test]
fn admitted_string_forms() {
    for opcode in [0xa4, 0xa5, 0xa6, 0xa7, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf] {
        check_length(&[opcode]);
        if opcode & 1 != 0 {
            check_length(&[0x66, opcode]);
        }
    }
    for (prefix, opcode) in [
        (0xf3, 0xa4),
        (0xf3, 0xa5),
        (0xf3, 0xaa),
        (0xf3, 0xab),
        (0xf3, 0xac),
        (0xf3, 0xad),
        (0xf2, 0xa6),
        (0xf2, 0xa7),
        (0xf2, 0xae),
        (0xf2, 0xaf),
        (0xf3, 0xa6),
        (0xf3, 0xa7),
        (0xf3, 0xae),
        (0xf3, 0xaf),
    ] {
        check_length(&[prefix, opcode]);
        if opcode & 1 != 0 {
            check_length(&[prefix, 0x66, opcode]);
        }
    }
}

#[test]
fn unsupported_string_forms_reject_even_with_zero_count() {
    for (prefix, opcode) in [
        (0xf0, 0xa5),
        (0xf2, 0xa4),
        (0xf2, 0xa5),
        (0xf2, 0xaa),
        (0xf2, 0xab),
        (0xf2, 0xac),
        (0xf2, 0xad),
    ] {
        let code = [prefix, opcode];
        assert_eq!(
            compile_block_from_bytes(0x1000, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode: prefix
            })
        );
        let mut image = Image::new(&code);
        image.cpu.registers.ecx = 0;
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "unsupported string form precedes count and operand access",
            Exit::Other(0x0008_0000_0000_1000 | (u64::from(prefix) << 32)),
        );
    }
}

test_cases!(single_element_transfer_forms, transfers());
test_cases!(comparison_operands_and_flags, comparisons());
test_cases!(faults_preserve_the_current_element, element_faults());
test_sequences!(direction_flags_and_consumers, histories());
