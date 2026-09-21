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
use wasm86_x86::{compile_block_from_bytes, Gpr32::*};

#[path = "near_control/linkage.rs"]
mod linkage;

#[rustfmt::skip]
fn relative_calls() -> Vec<Case> {
    [
        (0x1000, &[0xe8, 0x7f, 0, 0, 0][..], 0x1084, 0x9000, &[5, 0x10, 0, 0][..]),
        (0xffff_fffc, &[0xe8, 0, 0, 0, 0], 1, 0x9000, &[1, 0, 0, 0]),
        (0x1234_fffe, &[0x66, 0xe8, 0xfd, 0xff], 0xffff, 0x9002, &[2, 0]),
    ].into_iter().map(|(origin, code, target, stack, returned)| {
        Case::preserving_flags(format!("relative CALL target and saved fallthrough at {origin:08x}"), code)
            .at(origin).register(Esp, 0x9004, stack).dispatch(target)
            .memory(0x8fff, &[0xa5; 10], ReadWrite).expect_memory(stack, returned)
    }).collect()
}

#[rustfmt::skip]
fn register_targets() -> Vec<Case> {
    vec![
        Case::preserving_flags("CALL register saves fallthrough and dispatches its full target", &[0xff, 0xd0])
            .initial_register(Eax, 0x8123_4567).register(Esp, 0x9004, 0x9000).dispatch(0x8123_4567)
            .memory(0x9000, &[0xa5; 4], ReadWrite).expect_memory(0x9000, &[2, 0x10, 0, 0]),
        Case::preserving_flags("CALL word register zero-extends its target", &[0x66, 0xff, 0xd3])
            .initial_register(Ebx, 0x8123_8000).register(Esp, 0x9004, 0x9002).dispatch(0x8000)
            .memory(0x9002, &[0xa5; 2], ReadWrite).expect_memory(0x9002, &[3, 0x10]),
        Case::preserving_flags("CALL ESP captures the entry target before decrementing the stack", &[0xff, 0xd4])
            .register(Esp, 0, 0xffff_fffc).dispatch(0)
            .memory(0xffff_fffc, &[0xa5; 4], ReadWrite).expect_memory(0xffff_fffc, &[2, 0x10, 0, 0]),
        Case::preserving_flags("CALL SP captures the entry low half before decrementing full ESP", &[0x66, 0xff, 0xd4])
            .register(Esp, 0x1234_0000, 0x1233_fffe).dispatch(0)
            .memory(0x1233_fffe, &[0xa5; 2], ReadWrite).expect_memory(0x1233_fffe, &[3, 0x10]),
        Case::preserving_flags("JMP register preserves an unmapped stack", &[0xff, 0xe2])
            .initial_register(Edx, 0x8123_4567).dispatch(0x8123_4567),
        Case::preserving_flags("JMP word register zero-extends its target", &[0x66, 0xff, 0xe3])
            .initial_register(Ebx, 0x8123_8000).dispatch(0x8000),
    ]
}

#[rustfmt::skip]
fn returns() -> Vec<Case> {
    let mut cases: Vec<_> = [
        (&[0xc3][..], 0x9004, 0xf123_8001),
        (&[0x66, 0xc3], 0x9002, 0x8001),
        (&[0xc2, 0, 0], 0x9004, 0xf123_8001),
        (&[0x66, 0xc2, 0, 0], 0x9002, 0x8001),
        (&[0xc2, 0xff, 0xff], 0x0001_9003, 0xf123_8001),
    ].into_iter().map(|(code, stack, target)| {
        Case::preserving_flags(format!("RET width and unsigned cleanup: {code:02x?}"), code)
            .register(Esp, 0x9000, stack).dispatch(target)
            .memory(0x9000, &[1, 0x80, 0x23, 0xf1], ReadOnly)
    }).collect();
    cases.extend([
        Case::preserving_flags("word RET cleanup follows the pop without truncating full ESP", &[0x66, 0xc2, 0xff, 0xff])
            .register(Esp, 0x1234_fffe, 0x1235_ffff).dispatch(0xbeef).memory(0x1234_fffe, &[0xef, 0xbe], ReadOnly),
        Case::preserving_flags("RET reads a complete split return address across scattered pages", &[0xc3])
            .register(Esp, 0x4fff, 0x5003).dispatch(0xf123_8001)
            .map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly)
            .memory(0x4fff, &[1, 0x80, 0x23, 0xf1], ReadOnly),
    ]);
    cases
}

#[rustfmt::skip]
fn memory_targets() -> Vec<Case> {
    vec![
        Case::preserving_flags("CALL memory uses entry ESP before decrement", &[0xff, 0x14, 0x24])
            .register(Esp, 0x5004, 0x5000).dispatch(0x1234_5678)
            .memory(0x5000, &[0xa5, 0xa5, 0xa5, 0xa5, 0x78, 0x56, 0x34, 0x12], ReadWrite)
            .expect_memory(0x5000, &[3, 0x10, 0, 0]),
        Case::preserving_flags("word CALL reads its target before overwriting the same slot", &[0x66, 0xff, 0x54, 0x24, 0xfe])
            .register(Esp, 0x5004, 0x5002).dispatch(0x8001)
            .memory(0x5002, &[1, 0x80, 0xa5, 0xa5], ReadWrite).expect_memory(0x5002, &[5, 0x10]),
        Case::preserving_flags("CALL source and return slot alias one physical frame", &[0xff, 0x13])
            .initial_register(Ebx, 0x6000).register(Esp, 0x9004, 0x9000).dispatch(0x1234_5678)
            .map_page(6, 0xb000, ReadWrite).map_page(9, 0xb000, ReadWrite)
            .backing(0xb000, &[0x78, 0x56, 0x34, 0x12, 0xa5]).expect_memory(0x9000, &[2, 0x10, 0, 0]),
        Case::preserving_flags("CALL reads and writes complete split operands", &[0xff, 0x13])
            .initial_register(Ebx, 0x6fff).register(Esp, 0x5003, 0x4fff).dispatch(0xf123_8001)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .map_page(6, 0xb000, ReadOnly).map_page(7, 0xe000, ReadOnly)
            .memory(0x4fff, &[0xa5; 4], ReadWrite).memory(0x6fff, &[1, 0x80, 0x23, 0xf1], ReadOnly)
            .expect_memory(0x4fff, &[2, 0x10, 0, 0]),
        Case::preserving_flags("JMP memory reads through ESP without changing it", &[0xff, 0x24, 0x24])
            .initial_register(Esp, 0x5004).dispatch(0x1234_5678)
            .memory(0x5004, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags("word JMP uses a full source address and zero-extends its target", &[0x66, 0xff, 0x23])
            .initial_register(Ebx, 0x1234_6ffe).dispatch(0x8001).memory(0x1234_6ffe, &[1, 0x80], ReadOnly),
    ]
}

#[rustfmt::skip]
fn fault_ordering() -> Vec<Case> {
    vec![
        Case::preserving_flags("relative CALL cannot partially write its return slot", &[0x66, 0xe8, 0x7f, 0])
            .initial_register(Esp, 0x5001).memory(0x4fff, &[0xa5], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("CALL ESP preserves its entry target and pointer on a protected stack", &[0xff, 0xd4])
            .initial_register(Esp, 0x5004).memory(0x5000, &[0xa5; 4], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("CALL source must be complete before return-slot protection is checked", &[0x66, 0xff, 0x13])
            .initial_registers(&[(Ebx, 0x6fff), (Esp, 0x5001)])
            .memory(0x6fff, &[1], ReadOnly).memory(0x4fff, &[0xa5], ReadOnly).fault(0x7000, 0),
        Case::preserving_flags("RET missing source preserves the entry stack pointer", &[0xc3])
            .initial_register(Esp, 0x6000).fault(0x6000, 0),
    ]
}

fn add_checkpoint() -> Step {
    Step::new(
        &[0x01, 0xd0],
        Flags {
            cf: Clear,
            pf: Set,
            af: Set,
            zf: Clear,
            sf: Set,
            of: Set,
        },
    )
    .register(Eax, 0x8000_0000)
}

fn arithmetic_case(name: &str) -> Sequence {
    Sequence::from_opaque_flags(name).initial_registers(&[
        (Eax, 0x7fff_fffe),
        (Edx, 2),
        (Esp, 0x9004),
    ])
}

#[rustfmt::skip]
fn completed_state_cases() -> Vec<Sequence> {
    vec![
        arithmetic_case("relative CALL publishes pending flags, ESP and return address")
            .memory(0x8fff, &[0xa5; 10], ReadWrite)
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xbc, 4, 0x90, 0, 0]).register(Esp, 0x9004))
            .step(Step::preserving_flags(&[0xe8, 0x7f, 0, 0, 0]).register(Esp, 0x9000)
                .expect_memory(0x9000, &[0x0c, 0x10, 0, 0]).dispatch(0x108b))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        arithmetic_case("word indirect CALL reads the pending low accumulator and preserves its high half")
            .memory(0x9000, &[0xa5; 6], ReadWrite)
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0x66, 0xff, 0xd0]).register(Esp, 0x9002)
                .expect_memory(0x9002, &[5, 0x10]).dispatch(0))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        arithmetic_case("memory CALL uses pending ESP and captures an overlapping source before pushing")
            .memory(0x5000, &[0x78, 0x56, 0x34, 0x12, 0xa5], ReadWrite)
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xbc, 4, 0x50, 0, 0]).register(Esp, 0x5004))
            .step(Step::preserving_flags(&[0xff, 0x54, 0x24, 0xfc]).register(Esp, 0x5000)
                .expect_memory(0x5000, &[0x0b, 0x10, 0, 0]).dispatch(0x1234_5678))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        arithmetic_case("indirect JMP reads a pending target without touching the stack")
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xff, 0xe0]).dispatch(0x8000_0000))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        arithmetic_case("memory JMP uses a pending address and preserves pending flags")
            .memory(0x6000, &[1, 0x80, 0x23, 0xf1], ReadOnly)
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xbb, 0, 0x60, 0, 0]).register(Ebx, 0x6000))
            .step(Step::preserving_flags(&[0xff, 0x23]).dispatch(0xf123_8001))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        arithmetic_case("RET reads a prior PUSH and applies unsigned cleanup after pending arithmetic")
            .memory(0x9000, &[0xa5; 8], ReadWrite)
            .step(Step::preserving_flags(&[0x68, 0x78, 0x56, 0x34, 0x12]).register(Esp, 0x9000)
                .expect_memory(0x9000, &[0x78, 0x56, 0x34, 0x12]))
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xc2, 0xff, 0xff]).register(Esp, 0x0001_9003).dispatch(0x1234_5678))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
    ]
}

#[rustfmt::skip]
fn fault_publication_cases() -> Vec<Sequence> {
    let mut cases = Vec::new();
    for (name, source_present, address, error) in [
        ("source absent before protected return slot", false, 0x6000, 0),
        ("return-slot second page read-only", true, 0x5000, 3),
    ] {
        let mut case = arithmetic_case(name)
            .initial_register(Ebx, 0x6000).map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadOnly)
            .backing(0x8ffe, &[0xa5, 0x11]).backing(0xa000, &[0x22, 0x33, 0x44, 0x5a])
            .backing(0xb000, &[1, 0x80, 0x23, 0xf1])
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xbc, 3, 0x50, 0, 0]).register(Esp, 0x5003))
            .step(Step::preserving_flags(&[0xff, 0x13]).fault(address, error))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1);
        if source_present { case = case.map_page(6, 0xb000, ReadOnly); }
        cases.push(case);
    }
    cases.push(arithmetic_case("word RET read fault preserves prior ESP and arithmetic without cleanup")
        .map_page(4, 0x8000, ReadOnly).backing(0x8fff, &[0xef]).backing(0xa000, &[0xbe, 0xa5])
        .step(add_checkpoint())
        .step(Step::preserving_flags(&[0xbc, 0xff, 0x4f, 0, 0]).register(Esp, 0x4fff))
        .step(Step::preserving_flags(&[0x66, 0xc2, 0xff, 0xff]).fault(0x5000, 0))
        .trailing_code(&[0xb8, 0, 0, 0, 0], 1));
    cases.push(arithmetic_case("indirect JMP target fault preserves completed pending address and flags")
        .map_page(6, 0xb000, ReadOnly).backing(0xbfff, &[1])
        .step(add_checkpoint())
        .step(Step::preserving_flags(&[0xbb, 0xff, 0x6f, 0, 0]).register(Ebx, 0x6fff))
        .step(Step::preserving_flags(&[0xff, 0x23]).fault(0x7000, 0))
        .trailing_code(&[0xb8, 0, 0, 0, 0], 1));
    cases
}

#[test]
fn snapshots_require_complete_forms_and_stop_at_the_transfer() {
    for code in [
        &[0xe8, 0x66, 0xe8, 0xff, 0xc3][..],
        &[0x66, 0xe8, 0xc2, 0xff],
        &[0xff, 0xd4],
        &[0x66, 0xff, 0xd0],
        &[0xff, 0x10],
        &[0x66, 0xff, 0x54, 0x8c, 0x80],
        &[0xff, 0xe7],
        &[0x66, 0xff, 0xe4],
        &[0xff, 0x20],
        &[0x66, 0xff, 0x25, 0xc3, 0xc2, 0xff, 0xe8],
        &[0xc3],
        &[0x66, 0xc3],
        &[0xc2, 0xe8, 0xff],
        &[0x66, 0xc2, 0xc3, 0xff],
    ] {
        let complete = check_length(code);
        let trailing = [code, &[0xf4, 0x66, 0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, u32::MAX)
                .unwrap()
                .bytes,
            complete.bytes,
            "{code:02x?}"
        );
    }
}

test_cases!(relative_targets_and_return_addresses, relative_calls());
test_cases!(register_targets_use_original_values, register_targets());
test_cases!(return_width_and_unsigned_cleanup, returns());
test_cases!(entry_stack_and_overlapping_targets, memory_targets());
test_cases!(faults_preserve_uncompleted_transfers, fault_ordering());
test_sequences!(
    completed_registers_flags_and_stack_effects,
    completed_state_cases()
);
test_sequences!(faults_preserve_prior_completion, fault_publication_cases());
