use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{self, ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Ebx, Esp};

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
fn call_write_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack) in [
        (&[0xe8, 0x7f, 0, 0, 0][..], 0x5003), (&[0xff, 0xd4], 0x5003), (&[0xff, 0x13], 0x5003),
        (&[0x66, 0xe8, 0x7f, 0], 0x5001), (&[0x66, 0xff, 0xd4], 0x5001), (&[0x66, 0xff, 0x13], 0x5001),
    ] {
        for fault in &WRITE_FAULTS {
            let mut case = Case::preserving_flags(format!("CALL {code:02x?}, {} leaves return slot and ESP unchanged", fault.name), code)
                .initial_register(Esp, stack).initial_register(Ebx, 0x6000).map_page(6, 0xc000, ReadOnly)
                .backing(0xc000, &[1, 0x80, 0x23, 0xf1]).backing(0x8ffe, &[0xa5, 0x11])
                .backing(0xa000, &[0x22, 0x33, 0x44, 0x5a]).fault(fault.address, fault.error);
            for &(page, frame, permissions) in fault.mappings { case = case.map_page(page, frame, permissions); }
            cases.push(case);
        }
    }
    for (code, stack) in [(&[0xe8, 0, 0, 0, 0][..], 0x5004), (&[0x66, 0xe8, 0, 0], 0x5002)] {
        for (present, error) in [(false, 2), (true, 3)] {
            let mut case = Case::preserving_flags(format!("aligned CALL push fault {error}: {code:02x?}"), code)
                .initial_register(Esp, stack).backing(0xa000, &[0x11, 0x22, 0x33, 0x44]).fault(0x5000, error);
            if present { case = case.map_page(5, 0xa000, ReadOnly); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn target_read_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack) in [
        (&[0xff, 0x13][..], 0x9004), (&[0x66, 0xff, 0x13], 0x9004),
        (&[0xff, 0x23], 0x9004), (&[0x66, 0xff, 0x23], 0x9004),
        (&[0xc3], 0x4fff), (&[0x66, 0xc3], 0x4fff),
        (&[0xc2, 0xff, 0xff], 0x4fff), (&[0x66, 0xc2, 0xff, 0xff], 0x4fff),
    ] {
        for (first_present, address) in [(false, 0x4fff), (true, 0x5000)] {
            let mut case = Case::preserving_flags(format!("near transfer {code:02x?} target read fails at {address:04x}"), code)
                .initial_register(Esp, stack).initial_register(Ebx, 0x4fff).map_page(9, 0xb000, ReadWrite)
                .backing(0x8ffe, &[0xa5, 0x11]).backing(0xa000, &[0x22, 0x33, 0x44, 0x5a])
                .backing(0xb000, &[0x5a; 8]).fault(address, 0);
            if first_present { case = case.map_page(4, 0x8000, ReadOnly); }
            cases.push(case);
        }
    }
    cases.push(Case::preserving_flags("aligned indirect CALL source fault does not push", &[0xff, 0x13])
        .initial_register(Ebx, 0x6000).initial_register(Esp, 0x9004)
        .memory(0x9000, &[0xa5; 4], ReadWrite).fault(0x6000, 0));
    cases.push(Case::preserving_flags("aligned word RET fault does not release cleanup bytes", &[0x66, 0xc2, 0xff, 0xff])
        .initial_register(Esp, 0x6000).fault(0x6000, 0));
    cases
}

#[rustfmt::skip]
fn ordering_and_wrap_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [&[0xff, 0x13][..], &[0x66, 0xff, 0x13]] {
        for (first_present, address) in [(false, 0x6fff), (true, 0x7000)] {
            let mut case = Case::preserving_flags(format!("CALL source fault precedes return-slot protection fault: {code:02x?}, {address:04x}"), code)
                .initial_register(Ebx, 0x6fff).initial_register(Esp, 0x5001)
                .map_page(4, 0x8000, ReadOnly).backing(0x8ffc, &[0xa5; 4])
                .backing(0xbfff, &[1]).fault(address, 0);
            if first_present { case = case.map_page(6, 0xb000, ReadOnly); }
            cases.push(case);
        }
    }
    for (code, stack) in [(&[0xe8, 0, 0, 0, 0][..], 2), (&[0x66, 0xe8, 0, 0], 1)] {
        cases.push(Case::preserving_flags(format!("CALL wrapped push reaches an absent page zero: {code:02x?}"), code)
            .initial_register(Esp, stack).map_page(0xfffff, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x11, 0x22]).backing(0xa000, &[0x33, 0x44]).fault(0, 2));
    }
    for (code, source, stack) in [
        (&[0xff, 0x13][..], 0xffff_fffe, 0x9004), (&[0x66, 0xff, 0x13], 0xffff_ffff, 0x9004),
        (&[0xff, 0x23], 0xffff_fffe, 0x9004), (&[0x66, 0xff, 0x23], 0xffff_ffff, 0x9004),
        (&[0xc3], 0xffff_fffe, 0xffff_fffe), (&[0x66, 0xc2, 0xff, 0xff], 0xffff_ffff, 0xffff_ffff),
    ] {
        cases.push(Case::preserving_flags(format!("near target wrapped read reaches an absent page zero: {code:02x?}"), code)
            .initial_register(Ebx, source).initial_register(Esp, stack)
            .map_page(0xfffff, 0x8000, ReadOnly).map_page(9, 0xb000, ReadWrite)
            .backing(0x8ffe, &[0x11, 0x22]).backing(0xa000, &[0x33, 0x44]).backing(0xb000, &[0x5a; 8]).fault(0, 0));
    }
    cases
}

test_cases!(call_return_slot_atomicity, call_write_faults());
test_cases!(
    target_and_return_address_read_atomicity,
    target_read_faults()
);
test_cases!(
    source_fault_priority_and_wrapped_page_faults,
    ordering_and_wrap_faults()
);
