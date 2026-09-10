use wasm86_x86::{compile_block_from_bytes, Gpr32};

use crate::support::{
    machine::{both, expected, Exit, Image, Step},
    step::TestModule,
};

use super::image;

#[test]
fn memory_forms_exchange_exact_widths_across_scattered_pages() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        address: u32,
        register: Gpr32,
        initial_bytes: &'static [u8],
        stored_bytes: &'static [u8],
        result: u32,
    }
    for case in [
        Case {
            name: "high byte at the last mapped byte",
            code: &[0x86, 0x23],
            address: 0x4fff,
            register: Gpr32::Eax,
            initial_bytes: &[0x80],
            stored_bytes: &[0x22],
            result: 0x4433_8011,
        },
        Case {
            name: "unaligned word preserves the destination's upper half",
            code: &[0x66, 0x87, 0x3b],
            address: 0x4011,
            register: Gpr32::Edi,
            initial_bytes: &[0x80, 0xfe],
            stored_bytes: &[0x88, 0x88],
            result: 0x8888_fe80,
        },
        Case {
            name: "aligned dword",
            code: &[0x87, 0x13],
            address: 0x4020,
            register: Gpr32::Edx,
            initial_bytes: &[0x78, 0x56, 0x34, 0x92],
            stored_bytes: &[0x99, 0xaa, 0xbb, 0xcc],
            result: 0x9234_5678,
        },
        Case {
            name: "unaligned dword",
            code: &[0x87, 0x23],
            address: 0x4013,
            register: Gpr32::Esp,
            initial_bytes: &[0x78, 0x56, 0x34, 0x92],
            stored_bytes: &[0x55, 0x55, 0x55, 0x55],
            result: 0x9234_5678,
        },
        Case {
            name: "word across noncontiguous physical pages",
            code: &[0x66, 0x87, 0x2b],
            address: 0x4fff,
            register: Gpr32::Ebp,
            initial_bytes: &[0x80, 0xfe],
            stored_bytes: &[0x66, 0x66],
            result: 0x6666_fe80,
        },
        Case {
            name: "dword across noncontiguous physical pages",
            code: &[0x87, 0x03],
            address: 0x4ffe,
            register: Gpr32::Eax,
            initial_bytes: &[0x78, 0x56, 0x34, 0x92],
            stored_bytes: &[0x11, 0x22, 0x33, 0x44],
            result: 0x9234_5678,
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.ebx = case.address;
        image.map(4, 0x8000, true);
        let physical = 0x8000 + (case.address & 0xfff);
        let first_len = case
            .initial_bytes
            .len()
            .min((0x5000 - case.address) as usize);
        image.data(physical - 1, &[0x5a]);
        image.data(physical, &case.initial_bytes[..first_len]);
        let mut writes = vec![(physical, &case.stored_bytes[..first_len])];
        if first_len < case.initial_bytes.len() {
            image.map(5, 0xa000, true);
            image.data(0xa000, &case.initial_bytes[first_len..]);
            image.data(
                0xa000 + (case.initial_bytes.len() - first_len) as u32,
                &[0x5a],
            );
            writes.push((0xa000, &case.stored_bytes[first_len..]));
        } else if case.address + (first_len as u32) < 0x5000 {
            image.data(physical + first_len as u32, &[0x5a]);
        }
        let mut cpu = image.cpu;
        cpu.registers[case.register] = case.result;
        cpu.eip += case.code.len() as u32;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            case.name,
            case.code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &writes,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn exchanged_base_and_index_registers_use_the_old_effective_address() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        register: Gpr32,
        old: u32,
        result: u32,
        stored_bytes: &'static [u8],
    }
    for case in [
        Case {
            name: "dword exchange uses its old EAX base",
            code: &[0x87, 0x00],
            register: Gpr32::Eax,
            old: 0x8000_4010,
            result: 0x9234_5678,
            stored_bytes: &[0x10, 0x40, 0, 0x80],
        },
        Case {
            name: "word exchange retains the full old EAX base",
            code: &[0x66, 0x87, 0x00],
            register: Gpr32::Eax,
            old: 0x8000_4010,
            result: 0x8000_5678,
            stored_bytes: &[0x10, 0x40],
        },
        Case {
            name: "AH exchange cannot change its EAX base early",
            code: &[0x86, 0x20],
            register: Gpr32::Eax,
            old: 0x8000_4010,
            result: 0x8000_7810,
            stored_bytes: &[0x40],
        },
        Case {
            name: "scaled ECX index is evaluated before ECX changes",
            code: &[0x87, 0x4c, 0x8b, 0x10],
            register: Gpr32::Ecx,
            old: 4,
            result: 0x9234_5678,
            stored_bytes: &[4, 0, 0, 0],
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.ebx = 0x8000_3ff0;
        image.cpu.registers[case.register] = case.old;
        image.map(0x80004, 0x8000, true);
        image.data(0x800f, &[0x5a, 0x78, 0x56, 0x34, 0x92, 0x5a]);
        let mut cpu = image.cpu;
        cpu.registers[case.register] = case.result;
        cpu.eip += case.code.len() as u32;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            case.name,
            case.code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8010, case.stored_bytes)],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn write_faults_leave_both_operands_flags_and_count_unchanged() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        address: u32,
        first_writable: Option<bool>,
        second_writable: Option<bool>,
        fault_address: u32,
        error: u16,
    }
    for case in [
        Case {
            name: "missing byte page is a write fault",
            code: &[0x86, 0x03],
            address: 0x4020,
            first_writable: None,
            second_writable: None,
            fault_address: 0x4020,
            error: 2,
        },
        Case {
            name: "equal values still require write permission",
            code: &[0x87, 0x03],
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Case {
            name: "missing second dword page",
            code: &[0x87, 0x03],
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0x5000,
            error: 2,
        },
        Case {
            name: "read-only second dword page",
            code: &[0x87, 0x03],
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "read-only second word page",
            code: &[0x66, 0x87, 0x03],
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "word range cannot wrap",
            code: &[0x66, 0x87, 0x03],
            address: 0xffff_ffff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_ffff,
            error: 2,
        },
        Case {
            name: "dword range cannot wrap",
            code: &[0x87, 0x03],
            address: 0xffff_fffd,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_fffd,
            error: 2,
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.ebx = case.address;
        if let Some(writable) = case.first_writable {
            image.map(case.address >> 12, 0x8000, writable);
        }
        if let Some(writable) = case.second_writable {
            image.map(5, 0xa000, writable);
        }
        image.data(0x801f, &[0x5a, 0x11, 0x22, 0x33, 0x44, 0x5a]);
        image.data(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]);
        image.data(0xa000, &[0x92, 0x5a]);
        both(
            TestModule::interpreter(),
            case.name,
            case.code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: case.fault_address,
                    error: case.error,
                },
            }],
        );
    }
}

const EXCHANGES_THEN_FAULT: &[u8] = &[
    0x86, 0xc4, // XCHG AH,AL
    0x66, 0x92, // XCHG AX,DX
    0x87, 0x03, // XCHG [EBX],EAX
    0x86, 0x20, // XCHG [EAX],AH, using the old EAX
    0x66, 0x87, 0x11, // XCHG [ECX],DX, through a physical alias
    0x87, 0x00, // XCHG [EAX],EAX, now on a missing page
];
const DWORD_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0x99, 0xaa, 0x33, 0x44])];
const BYTE_WRITE: &[(u32, &[u8])] = &[(0x8010, &[0x40])];
const WORD_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0x22, 0x11])];

fn exchanges_then_fault() -> (Image, Vec<Step<'static>>) {
    let mut image = image(EXCHANGES_THEN_FAULT);
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ecx = 0x6000;
    image.cpu.instruction_count = 0xffff_fffd;
    image.map(4, 0x8000, true);
    image.map(6, 0x8000, true);
    image.data(0x7fff, &[0x5a, 0x10, 0x40, 0, 0, 0x5a]);
    image.data(0x800f, &[0x5a, 0x80, 0x5a]);
    let mut cpu = image.cpu;
    let mut steps = Vec::new();
    for (next, eax, edx, writes) in [
        (0x1002, 0x4433_1122, 0xccbb_aa99, &[][..]),
        (0x1004, 0x4433_aa99, 0xccbb_1122, &[][..]),
        (0x1006, 0x0000_4010, 0xccbb_1122, DWORD_WRITE),
        (0x1008, 0x0000_8010, 0xccbb_1122, BYTE_WRITE),
        (0x100b, 0x0000_8010, 0xccbb_aa99, WORD_WRITE),
    ] {
        cpu.registers.eax = eax;
        cpu.registers.edx = edx;
        cpu.eip = next;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        steps.push(Step {
            cpu,
            ram: writes,
            exit: Exit::Dispatch(next),
        });
    }
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x8010,
            error: 2,
        },
    });
    (image, steps)
}

#[test]
fn sequential_register_and_memory_aliases_publish_before_a_later_fault() {
    let (image, steps) = exchanges_then_fault();
    both(
        TestModule::interpreter(),
        "completed exchanges survive a later write fault",
        EXCHANGES_THEN_FAULT,
        6,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn exchanges_and_fault_publication_execute_in_optimizing_v8() {
    let (image, steps) = exchanges_then_fault();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        expected(&image, &steps),
        "interpreter",
    );
    let block =
        TestModule::new(&compile_block_from_bytes(image.cpu.eip, EXCHANGES_THEN_FAULT, 6).unwrap());
    let writes = [DWORD_WRITE, BYTE_WRITE, WORD_WRITE].concat();
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: steps.last().unwrap().cpu,
                ram: &writes,
                exit: steps.last().unwrap().exit,
            }]
        ),
        "snapshot block",
    );
}
