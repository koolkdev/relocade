use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32;

use super::LAZY_FLAGS;

#[rustfmt::skip]
fn width_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    struct MemoryExchange {
        name: &'static str,
        code: &'static [u8],
        address: u32,
        register: Gpr32,
        initial_bytes: &'static [u8],
        stored_bytes: &'static [u8],
        input: u32,
        result: u32,
    }
    for case in [
        MemoryExchange {
            name: "high byte at the last mapped byte",
            code: &[0x86, 0x23],
            address: 0x4fff,
            register: Gpr32::Eax,
            initial_bytes: &[0x80],
            stored_bytes: &[0x22],
            input: 0x4433_2211,
            result: 0x4433_8011,
        },
        MemoryExchange {
            name: "unaligned word preserves the destination's upper half",
            code: &[0x66, 0x87, 0x3b],
            address: 0x4011,
            register: Gpr32::Edi,
            initial_bytes: &[0x80, 0xfe],
            stored_bytes: &[0x88, 0x88],
            input: 0x8888_8888,
            result: 0x8888_fe80,
        },
        MemoryExchange {
            name: "aligned dword",
            code: &[0x87, 0x13],
            address: 0x4020,
            register: Gpr32::Edx,
            input: 0xccbb_aa99,
            initial_bytes: &[0x78, 0x56, 0x34, 0x92],
            stored_bytes: &[0x99, 0xaa, 0xbb, 0xcc],
            result: 0x9234_5678,
        },
        MemoryExchange {
            name: "unaligned dword",
            code: &[0x87, 0x23],
            address: 0x4013,
            register: Gpr32::Esp,
            input: 0x5555_5555,
            initial_bytes: &[0x78, 0x56, 0x34, 0x92],
            stored_bytes: &[0x55, 0x55, 0x55, 0x55],
            result: 0x9234_5678,
        },
        MemoryExchange {
            name: "word across noncontiguous physical pages",
            code: &[0x66, 0x87, 0x2b],
            address: 0x4fff,
            register: Gpr32::Ebp,
            initial_bytes: &[0x80, 0xfe],
            stored_bytes: &[0x66, 0x66],
            input: 0x6666_6666,
            result: 0x6666_fe80,
        },
        MemoryExchange {
            name: "dword across noncontiguous physical pages",
            input: 0x4433_2211,
            code: &[0x87, 0x03],
            address: 0x4ffe,
            register: Gpr32::Eax,
            initial_bytes: &[0x78, 0x56, 0x34, 0x92],
            stored_bytes: &[0x11, 0x22, 0x33, 0x44],
            result: 0x9234_5678,
        },
    ] {
        let mut input = Case::preserving_flags(case.name, case.code).stored_flags(LAZY_FLAGS)
            .initial_register(Gpr32::Ebx, case.address).register(case.register, case.input, case.result)
            .map_page(4, 0x8000, ReadWrite);
        let first_len = case.initial_bytes.len().min((0x5000 - case.address) as usize);
        input = input.backing(0x8000 + (case.address & 0xfff) - 1, &[0x5a]);
        if first_len < case.initial_bytes.len() {
            input = input.map_page(5, 0xa000, ReadWrite)
                .backing(0xa000 + (case.initial_bytes.len() - first_len) as u32, &[0x5a]);
        } else if case.address + (first_len as u32) < 0x5000 {
            input = input.backing(0x8000 + (case.address & 0xfff) + first_len as u32, &[0x5a]);
        }
        cases.push(input.memory(case.address, case.initial_bytes, ReadWrite).expect_memory(case.address, case.stored_bytes));
    }
    cases
}
test_cases!(
    memory_forms_exchange_exact_widths_across_scattered_pages,
    width_cases()
);

#[rustfmt::skip]
fn address_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    struct Exchange {
        name: &'static str,
        code: &'static [u8],
        register: Gpr32,
        old: u32,
        result: u32,
        stored_bytes: &'static [u8],
    }
    for case in [
        Exchange {
            name: "dword exchange uses its old EAX base",
            code: &[0x87, 0x00],
            register: Gpr32::Eax,
            old: 0x8000_4010,
            result: 0x9234_5678,
            stored_bytes: &[0x10, 0x40, 0, 0x80],
        },
        Exchange {
            name: "word exchange retains the full old EAX base",
            code: &[0x66, 0x87, 0x00],
            register: Gpr32::Eax,
            old: 0x8000_4010,
            result: 0x8000_5678,
            stored_bytes: &[0x10, 0x40],
        },
        Exchange {
            name: "AH exchange cannot change its EAX base early",
            code: &[0x86, 0x20],
            register: Gpr32::Eax,
            old: 0x8000_4010,
            result: 0x8000_7810,
            stored_bytes: &[0x40],
        },
        Exchange {
            name: "scaled ECX index is evaluated before ECX changes",
            code: &[0x87, 0x4c, 0x8b, 0x10],
            register: Gpr32::Ecx,
            old: 4,
            result: 0x9234_5678,
            stored_bytes: &[4, 0, 0, 0],
        },
    ] {
        cases.push(Case::preserving_flags(case.name, case.code).stored_flags(LAZY_FLAGS)
            .initial_register(Gpr32::Ebx, 0x8000_3ff0).register(case.register, case.old, case.result)
            .memory(0x8000_400f, &[0x5a, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite)
            .expect_memory(0x8000_4010, case.stored_bytes));
    }
    cases
}
test_cases!(
    exchanged_base_and_index_registers_use_the_old_effective_address,
    address_cases()
);

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    struct Fault {
        name: &'static str,
        code: &'static [u8],
        address: u32,
        first_writable: Option<bool>,
        second_writable: Option<bool>,
        fault_address: u32,
        error: u16,
    }
    for case in [
        Fault {
            name: "missing byte page is a write fault",
            code: &[0x86, 0x03],
            address: 0x4020,
            first_writable: None,
            second_writable: None,
            fault_address: 0x4020,
            error: 2,
        },
        Fault {
            name: "equal values still require write permission",
            code: &[0x87, 0x03],
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Fault {
            name: "missing second dword page",
            code: &[0x87, 0x03],
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0x5000,
            error: 2,
        },
        Fault {
            name: "read-only second dword page",
            code: &[0x87, 0x03],
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Fault {
            name: "read-only second word page",
            code: &[0x66, 0x87, 0x03],
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Fault {
            name: "word wrap reaches an absent page zero",
            code: &[0x66, 0x87, 0x03],
            address: 0xffff_ffff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0,
            error: 2,
        },
        Fault {
            name: "dword wrap reaches an absent page zero",
            code: &[0x87, 0x03],
            address: 0xffff_fffd,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0,
            error: 2,
        },
    ] {
        let mut input = Case::preserving_flags(case.name, case.code).stored_flags(LAZY_FLAGS)
            .initial_register(Gpr32::Ebx, case.address).initial_register(Gpr32::Eax, 0x4433_2211)
            .backing(0x801f, &[0x5a, 0x11, 0x22, 0x33, 0x44, 0x5a])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x92, 0x5a]);
        if let Some(writable) = case.first_writable {
            input = input.map_page(case.address >> 12, 0x8000, if writable { ReadWrite } else { ReadOnly });
        }
        if let Some(writable) = case.second_writable {
            input = input.map_page(5, 0xa000, if writable { ReadWrite } else { ReadOnly });
        }
        cases.push(input.fault(case.fault_address, case.error));
    }
    cases
}
test_cases!(
    write_faults_leave_both_operands_flags_and_count_unchanged,
    fault_cases()
);
