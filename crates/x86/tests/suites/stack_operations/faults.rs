use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{self, ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Ebx, Ecx, Esp};

struct WriteFault {
    name: &'static str,
    mappings: &'static [(u32, u32, Permissions)],
    address: u32,
    error: u16,
}

#[rustfmt::skip]
const WRITE_FAULTS: [WriteFault; 4] = [
    WriteFault { name: "absent first page", mappings: &[], address: 0x4fff, error: 2 },
    WriteFault { name: "read-only first page", mappings: &[(4, 0x8000, ReadOnly)], address: 0x4fff, error: 3 },
    WriteFault { name: "absent second page", mappings: &[(4, 0x8000, ReadWrite)], address: 0x5000, error: 2 },
    WriteFault { name: "read-only second page", mappings: &[(4, 0x8000, ReadWrite), (5, 0xa000, ReadOnly)], address: 0x5000, error: 3 },
];

#[rustfmt::skip]
fn push_write_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack) in [
        (&[0x50][..], 0x5003), (&[0xff, 0xf4], 0x5003), (&[0xff, 0x33], 0x5003),
        (&[0x68, 0x78, 0x56, 0x34, 0x12], 0x5003), (&[0x6a, 0x80], 0x5003),
        (&[0x66, 0x50], 0x5001), (&[0x66, 0xff, 0xf4], 0x5001), (&[0x66, 0xff, 0x33], 0x5001),
        (&[0x66, 0x68, 0x78, 0x56], 0x5001), (&[0x66, 0x6a, 0x80], 0x5001),
    ] {
        for fault in &WRITE_FAULTS {
            let mut case = Case::preserving_flags(format!("PUSH {code:02x?}: {} leaves ESP and RAM unchanged", fault.name), code)
                .initial_register(Esp, stack).initial_register(Ebx, 0x6000).map_page(6, 0xc000, ReadOnly)
                .backing(0xc000, &[0x78, 0x56, 0x34, 0x12]).backing(0x8ffe, &[0xa5, 0x11]).backing(0xa000, &[0x22, 0x33, 0x44, 0x5a])
                .fault(fault.address, fault.error);
            for &(page, frame, permissions) in fault.mappings { case = case.map_page(page, frame, permissions); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn read_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack, ebx) in [
        (&[0xff, 0x33][..], 0x9004, 0x4fff), (&[0x66, 0xff, 0x33], 0x9004, 0x4fff),
        (&[0x58], 0x4fff, 0x9000), (&[0x66, 0x58], 0x4fff, 0x9000),
        (&[0x5c], 0x4fff, 0x9000), (&[0x66, 0x5c], 0x4fff, 0x9000),
        (&[0x8f, 0x03], 0x4fff, 0x9000), (&[0x66, 0x8f, 0x03], 0x4fff, 0x9000),
    ] {
        for (first_present, fault_address) in [(false, 0x4fff), (true, 0x5000)] {
            let mut case = Case::preserving_flags(format!("stack transfer {code:02x?}: missing source at {fault_address:04x}"), code)
                .initial_register(Esp, stack).initial_register(Ebx, ebx).map_page(9, 0xb000, ReadWrite)
                .backing(0x8ffe, &[0xa5, 0x11]).backing(0xa000, &[0x22, 0x33, 0x44, 0x5a]).backing(0xb000, &[0x5a; 8])
                .fault(fault_address, 0);
            if first_present { case = case.map_page(4, 0x8000, ReadOnly); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn pop_write_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [&[0x8f, 0x03][..], &[0x66, 0x8f, 0x03]] {
        for fault in &WRITE_FAULTS {
            let mut case = Case::preserving_flags(format!("POP {code:02x?}: {} leaves original ESP and memory", fault.name), code)
                .initial_register(Esp, 0x9000).initial_register(Ebx, 0x4fff).map_page(9, 0xb000, ReadOnly)
                .backing(0xb000, &[0x78, 0x56, 0x34, 0x12]).backing(0x8ffe, &[0xa5, 0x11]).backing(0xa000, &[0x22, 0x33, 0x44, 0x5a])
                .fault(fault.address, fault.error);
            for &(page, frame, permissions) in fault.mappings { case = case.map_page(page, frame, permissions); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn prospective_esp_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack) in [
        (&[0x8f, 0x04, 0x24][..], 0x4ffc), (&[0x66, 0x8f, 0x04, 0x24], 0x4ffe),
        (&[0x8f, 0x44, 0x8c, 0xf4], 0x4ffc), (&[0x66, 0x8f, 0x44, 0x8c, 0xf4], 0x4ffe),
    ] {
        for (present, error) in [(false, 2), (true, 3)] {
            let mut case = Case::preserving_flags(format!("POP {code:02x?}: prospective ESP destination fault {error}"), code)
                .initial_register(Esp, stack).initial_register(Ecx, 3).map_page(4, 0x8000, ReadOnly)
                .backing(0x8ffc, &[0x78, 0x56, 0x34, 0x12]).backing(0xa000, &[0x5a; 4]).fault(0x5000, error);
            if present { case = case.map_page(5, 0xa000, ReadOnly); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn ordering_and_wrap_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack, fault_address) in [
        (&[0xff, 0x33][..], 0x5003, 0x6fff), (&[0x66, 0xff, 0x33], 0x5003, 0x6fff),
        (&[0x8f, 0x03], 0x4fff, 0x4fff), (&[0x66, 0x8f, 0x03], 0x4fff, 0x4fff),
    ] {
        cases.push(Case::preserving_flags(format!("stack transfer {code:02x?}: source fault precedes destination fault"), code)
            .initial_register(Esp, stack).initial_register(Ebx, 0x6fff).fault(fault_address, 0));
    }
    for (code, stack, address, error) in [
        (&[0x50][..], 2, 0xffff_fffe, 2), (&[0x66, 0x50], 1, 0xffff_ffff, 2),
        (&[0x58], 0xffff_fffe, 0xffff_fffe, 0), (&[0x66, 0x58], 0xffff_ffff, 0xffff_ffff, 0),
    ] {
        cases.push(Case::preserving_flags(format!("stack pointer wrap cannot make an operand span wrap: {code:02x?}"), code)
            .initial_register(Esp, stack).map_page(0xfffff, 0x8000, ReadWrite).map_page(0, 0xa000, ReadWrite)
            .backing(0x8ffe, &[0x11, 0x22]).backing(0xa000, &[0x33, 0x44]).fault(address, error));
    }
    cases
}

test_cases!(push_write_atomicity, push_write_faults());
test_cases!(source_read_atomicity, read_faults());
test_cases!(pop_write_atomicity, pop_write_faults());
test_cases!(pop_esp_address_atomicity, prospective_esp_faults());
test_cases!(
    source_fault_priority_and_nonwrapping_spans,
    ordering_and_wrap_faults()
);
