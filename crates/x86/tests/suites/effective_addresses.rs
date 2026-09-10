use wasm86_x86::{compile_block_from_bytes, Gpr32};
use wasmparser::{Parser, Payload, TypeRef};

use crate::support::{
    arithmetic,
    machine::{self, both, Exit, Image, Step},
    step::TestModule,
};

#[path = "effective_addresses/decoding.rs"]
mod decoding;

#[test]
fn base_index_and_displacement_forms_compute_addresses() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        registers: &'static [(Gpr32, u32)],
        destination: Gpr32,
        result: u32,
    }
    for case in [
        Case {
            name: "absolute displacement without SIB",
            code: &[0x8d, 0x3d, 0x78, 0x56, 0x34, 0x12],
            registers: &[],
            destination: Gpr32::Edi,
            result: 0x1234_5678,
        },
        Case {
            name: "SIB without base or index",
            code: &[0x8d, 0x34, 0x25, 0x98, 0xba, 0xdc, 0xfe],
            registers: &[],
            destination: Gpr32::Esi,
            result: 0xfedc_ba98,
        },
        Case {
            name: "scaled index without base",
            code: &[0x8d, 0x2c, 0xcd, 0x80, 0xff, 0xff, 0xff],
            registers: &[(Gpr32::Ecx, 0x22)],
            destination: Gpr32::Ebp,
            result: 0x90,
        },
        Case {
            name: "absent index ignores the scale bits",
            code: &[0x8d, 0x1c, 0xe3],
            registers: &[(Gpr32::Ebx, 0x8000_1234)],
            destination: Gpr32::Ebx,
            result: 0x8000_1234,
        },
        Case {
            name: "ESP base through SIB",
            code: &[0x8d, 0x24, 0x24],
            registers: &[(Gpr32::Esp, 0xdead_4000)],
            destination: Gpr32::Esp,
            result: 0xdead_4000,
        },
        Case {
            name: "EBP base requires a displacement byte",
            code: &[0x8d, 0x6d, 0],
            registers: &[(Gpr32::Ebp, 0x8765_4321)],
            destination: Gpr32::Ebp,
            result: 0x8765_4321,
        },
        Case {
            name: "negative disp8 wraps below zero",
            code: &[0x8d, 0x43, 0x80],
            registers: &[(Gpr32::Ebx, 0x10)],
            destination: Gpr32::Eax,
            result: 0xffff_ff90,
        },
        Case {
            name: "positive disp8 wraps above the final address",
            code: &[0x8d, 0x43, 0x7f],
            registers: &[(Gpr32::Ebx, 0xffff_fff0)],
            destination: Gpr32::Eax,
            result: 0x6f,
        },
        Case {
            name: "high-bit disp32 wraps the sum",
            code: &[0x8d, 0x83, 0, 0, 0, 0x80],
            registers: &[(Gpr32::Ebx, 0x8000_4000)],
            destination: Gpr32::Eax,
            result: 0x4000,
        },
        Case {
            name: "word destination keeps its upper half with 32-bit addressing",
            code: &[0x66, 0x8d, 0x44, 0x4b, 1],
            registers: &[
                (Gpr32::Eax, 0x4433_2211),
                (Gpr32::Ebx, 0x8000_ffff),
                (Gpr32::Ecx, 0x1234_0002),
            ],
            destination: Gpr32::Eax,
            result: 0x4433_0004,
        },
    ] {
        let mut image = arithmetic::image(case.code);
        for &(register, value) in case.registers {
            image.cpu.registers[register] = value;
        }
        let mut cpu = image.cpu;
        cpu.registers[case.destination] = case.result;
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
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn every_sib_scale_uses_a_wrapping_32_bit_index() {
    for (sib, result) in [
        (0x0b, 0x4000_0011),
        (0x4b, 0x8000_0012),
        (0x8b, 0x14),
        (0xcb, 0x18),
    ] {
        let code = [0x8d, 0x54, sib, 0x20]; // LEA EDX,[EBX+ECX*scale+0x20]
        let mut image = arithmetic::image(&code);
        image.cpu.registers.ebx = 0xffff_fff0;
        image.cpu.registers.ecx = 0x4000_0001;
        let mut cpu = image.cpu;
        cpu.registers.edx = result;
        cpu.eip = 0x1004;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            &format!("SIB {sib:02x}"),
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn computed_addresses_do_not_access_guest_data() {
    let code = [0x8d, 0x03]; // LEA EAX,[EBX]
    for (name, address, mapped) in [
        ("missing target page", 0x4fff, false),
        ("read-only target page", 0x4fff, true),
        (
            "final linear byte has no data-range check",
            0xffff_ffff,
            false,
        ),
    ] {
        let mut image = arithmetic::image(&code);
        image.cpu.registers.ebx = address;
        if mapped {
            image.map(4, 0x8000, false);
            image.data(0x8ffb, &[0x5a, 0x78, 0x56, 0x34, 0x12]);
        }
        let mut cpu = image.cpu;
        cpu.registers.eax = address;
        cpu.eip = 0x1002;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn snapshot_lea_imports_no_guest_data_or_page_table_memory() {
    for code in [&[0x8d, 0x03][..], &[0x66, 0x8d, 0x44, 0x8b, 0x80][..]] {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let mut memories = Vec::new();
        for payload in Parser::new(0).parse_all(&module.bytes) {
            if let Payload::ImportSection(imports) = payload.unwrap() {
                for import in imports {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Memory(_)) {
                        memories.push((import.module, import.name));
                    }
                }
            }
        }
        assert_eq!(memories, [("wasm86", "cpuState")]);
    }
}

const MIXED_ADDRESS_TRACE: &[u8] = &[
    0x66, 0xbb, 0xf0, 0xff, // MOV BX,0xfff0
    0xb1, 4, // MOV CL,4
    0x8d, 0x44, 0x8b, 0x20, // LEA EAX,[EBX+ECX*4+0x20]
    0x66, 0x8d, 0x64, 0x40, 0x80, // LEA SP,[EAX+EAX*2-0x80]
    0x8d, 0x24, 0x24, // LEA ESP,[ESP]
    0x8d, 0x4c, 0x8c, 0x10, // LEA ECX,[ESP+ECX*4+0x10]
    0x66, 0x8d, 0x49, 0xff, // LEA CX,[ECX-1]
    0x8d, 0x31, // LEA ESI,[ECX], with no target mapping
];

fn mixed_address_trace() -> (Image, Vec<Step<'static>>) {
    let mut image = arithmetic::image(MIXED_ADDRESS_TRACE);
    image.cpu.registers.eax = 0x1234_5678;
    image.cpu.registers.ebx = 0x1111_4000;
    image.cpu.registers.ecx = 0x8000_0108;
    image.cpu.registers.esp = 0x9000_0100;
    image.cpu.flags.kind = 9;
    image.cpu.flags.left = 0x7fff_fffe;
    image.cpu.flags.right = 0xffff_fffe;
    image.cpu.instruction_count = 0xffff_fffd;
    let mut cpu = image.cpu;
    let mut steps = Vec::new();
    for (next, destination, result) in [
        (0x1004, Gpr32::Ebx, 0x1111_fff0),
        (0x1006, Gpr32::Ecx, 0x8000_0104),
        (0x100a, Gpr32::Eax, 0x1112_0420),
        (0x100f, Gpr32::Esp, 0x9000_0be0),
        (0x1012, Gpr32::Esp, 0x9000_0be0),
        (0x1016, Gpr32::Ecx, 0x9000_1000),
        (0x101a, Gpr32::Ecx, 0x9000_0fff),
        (0x101c, Gpr32::Esi, 0x9000_0fff),
    ] {
        cpu.registers[destination] = result;
        cpu.eip = next;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(next),
        });
    }
    (image, steps)
}

#[test]
fn mixed_width_writes_feed_old_base_and_index_values() {
    let (image, steps) = mixed_address_trace();
    both(
        TestModule::interpreter(),
        "mixed address aliases",
        MIXED_ADDRESS_TRACE,
        8,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn effective_addresses_and_aliases_execute_in_optimizing_v8() {
    let (image, steps) = mixed_address_trace();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        machine::expected(&image, &steps),
        "interpreter",
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, MIXED_ADDRESS_TRACE, 8).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: steps.last().unwrap().cpu,
                ram: &[],
                exit: Exit::Dispatch(0x101c)
            }]
        ),
        "snapshot block",
    );
}
