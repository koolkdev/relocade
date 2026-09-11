use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::ReadWrite,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn transfer_sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("mixed word and dword transfers publish every completed instruction")
            .initial_registers(&[(Eax, 0x1234_5678), (Esp, 0x9004)])
            .map_page(8, 0x8000, ReadWrite).map_page(9, 0xa000, ReadWrite)
            .backing(0x8ffd, &[0xa5; 3]).backing(0xa000, &[0xa5; 5])
            .step(Step::preserving_flags(&[0x50]).register(Esp, 0x9000).expect_memory(0x9000, &[0x78, 0x56, 0x34, 0x12]))
            .step(Step::preserving_flags(&[0x66, 0x6a, 0x80]).register(Esp, 0x8ffe).expect_memory(0x8ffe, &[0x80, 0xff]))
            .step(Step::preserving_flags(&[0x66, 0x5a]).register(Edx, 0xdead_ff80).register(Esp, 0x9000))
            .step(Step::preserving_flags(&[0x5b]).register(Ebx, 0x1234_5678).register(Esp, 0x9004)),
        Case::preserving_flags("completed PUSH and POP effects survive a later POP destination fault")
            .initial_registers(&[(Eax, 0x1234_5678), (Ecx, 0x6000), (Esp, 0x5002)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .backing(0x8ffd, &[0xa5; 3]).backing(0xa000, &[0xa5, 0xa5, 0xbe, 0xad, 0x11, 0x22, 0x33, 0x44, 0x5a])
            .step(Step::preserving_flags(&[0x50]).register(Esp, 0x4ffe).expect_memory(0x4ffe, &[0x78, 0x56, 0x34, 0x12]))
            .step(Step::preserving_flags(&[0x66, 0x8f, 0x04, 0x24]).register(Esp, 0x5000).expect_memory(0x5000, &[0x78, 0x56]))
            .step(Step::preserving_flags(&[0x5a]).register(Edx, 0xadbe_5678).register(Esp, 0x5004))
            .step(Step::preserving_flags(&[0x8f, 0x01]).fault(0x6000, 2)),
    ]
}

test_sequences!(mixed_transfers_and_faults, transfer_sequences());

#[rustfmt::skip]
fn arithmetic_before_pop() -> Vec<Case> {
    use crate::support::cases::Permissions::{ReadOnly, ReadWrite};
    struct Scenario { name: &'static str, source: bool, destination: Option<crate::support::cases::Permissions>, fault: Option<(u32, u16)> }
    let mut cases = Vec::new();
    for scenario in [
        Scenario { name: "source missing", source: false, destination: Some(ReadWrite), fault: Some((0x4ffe, 0)) },
        Scenario { name: "destination missing", source: true, destination: None, fault: Some((0x5000, 2)) },
        Scenario { name: "destination read-only", source: true, destination: Some(ReadOnly), fault: Some((0x5000, 3)) },
        Scenario { name: "success", source: true, destination: Some(ReadWrite), fault: None },
    ] {
        let mut case = Case::from_opaque_flags(format!("POP address guards preserve completed arithmetic: {}", scenario.name))
            .initial_registers(&[(Eax, 0x7fff_fffe), (Edx, 2), (Esp, 0x9000), (Esi, 3)])
            .backing(0x8ffd, &[0x5a, 0x78, 0x56]).backing(0xa000, &[0xa5; 4])
            .step(Step::new(&[0x01, 0xd0], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0xbc, 0xfe, 0x4f, 0, 0]).register(Esp, 0x4ffe));
        if scenario.source { case = case.map_page(4, 0x8000, ReadOnly); }
        if let Some(permissions) = scenario.destination { case = case.map_page(5, 0xa000, permissions); }
        if let Some((address, error)) = scenario.fault {
            case = case.step(Step::preserving_flags(&[0x66, 0x8f, 0x44, 0xb4, 0xf4]).fault(address, error))
                .trailing_code(&[0x89, 0xe7], 1);
        } else {
            case = case.step(Step::preserving_flags(&[0x66, 0x8f, 0x44, 0xb4, 0xf4]).register(Esp, 0x5000).expect_memory(0x5000, &[0x78, 0x56]))
                .step(Step::preserving_flags(&[0x89, 0xe7]).register(Edi, 0x5000));
        }
        cases.push(case);
    }
    cases
}

test_sequences!(
    completed_arithmetic_and_stack_addresses,
    arithmetic_before_pop()
);
