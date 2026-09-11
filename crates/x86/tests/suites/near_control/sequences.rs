use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

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

fn arithmetic_case(name: &str) -> Case {
    Case::from_opaque_flags(name)
        .instruction_count(0xffff_fffe)
        .initial_registers(&[(Eax, 0x7fff_fffe), (Edx, 2), (Esp, 0x9004)])
}

#[rustfmt::skip]
fn completed_state_cases() -> Vec<Case> {
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
fn fault_publication_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, source_present, second_stack, address, error) in [
        ("source absent before protected return slot", false, Some(ReadOnly), 0x6000, 0),
        ("return-slot second page absent", true, None, 0x5000, 2),
        ("return-slot second page read-only", true, Some(ReadOnly), 0x5000, 3),
    ] {
        let mut case = arithmetic_case(name)
            .initial_register(Ebx, 0x6000).map_page(4, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0xa5, 0x11]).backing(0xa000, &[0x22, 0x33, 0x44, 0x5a])
            .backing(0xb000, &[1, 0x80, 0x23, 0xf1])
            .step(add_checkpoint())
            .step(Step::preserving_flags(&[0xbc, 3, 0x50, 0, 0]).register(Esp, 0x5003))
            .step(Step::preserving_flags(&[0xff, 0x13]).fault(address, error))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1);
        if source_present { case = case.map_page(6, 0xb000, ReadOnly); }
        if let Some(permissions) = second_stack { case = case.map_page(5, 0xa000, permissions); }
        cases.push(case);
    }
    cases.push(arithmetic_case("word RET read fault preserves prior ESP, arithmetic, and count without cleanup")
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

test_sequences!(
    completed_registers_flags_and_stack_effects,
    completed_state_cases()
);
test_sequences!(faults_preserve_prior_completion, fault_publication_cases());
