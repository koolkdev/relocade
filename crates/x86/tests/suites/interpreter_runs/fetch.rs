//! Live instruction fetch across mappings, prefixes and execution entries.

use super::check;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{CallPatches, Engine, TestModule},
};

pub(super) fn check_fetch(engine: Engine, module: &TestModule) {
    completed_progress_at_faults(engine, module);
    live_bytes_and_prefixes(engine, module);
    extended_memory_fields(engine, module);
    for next_frame in [0x4000, 0x6000] {
        code_pages(engine, module, 0, next_frame);
    }
    mappings_between_entries(engine, module);
}

pub(super) fn segmented_code_pages(engine: Engine, module: &TestModule) {
    code_pages(engine, module, 0x3123, 0x6000);
}

fn code_pages(engine: Engine, module: &TestModule, cs_base: u32, next_frame: u32) {
    let code = [
        0xb8, 42, 0, 0, 0, // MOV EAX,42
        0xb9, 0x78, 0x56, 0x34, 0x12, // MOV ECX,12345678 across the page boundary
        0xba, 0xef, 0xcd, 0xab, 0x89, // MOV EDX,89ABCDEF on the next page
        0xeb, 0,
    ];
    let mut image = Image::empty();
    image.cpu.eip = 0x1ff8 - (cs_base & 0xfff);
    image.cpu.segments.cs.base = cs_base;
    let first_page = (cs_base + image.cpu.eip) >> 12;
    image.map(first_page, 0x3000, false);
    image.map(first_page + 1, next_frame, false);
    image.data(0x3ff8, &code[..8]);
    image.data(next_frame, &code[8..]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 42;
    cpu.registers.ecx = 0x1234_5678;
    cpu.registers.edx = 0x89ab_cdef;
    cpu.eip = image.cpu.eip + code.len() as u32;
    cpu.instruction_count = 3;
    check(
        engine,
        module,
        "instruction progress crosses linear code-page mappings",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

fn extended_memory_fields(engine: Engine, module: &TestModule) {
    let mut image = Image::new(&[
        0x0f, 0xb6, 0x44, 0x4b, 3, // MOVZX EAX,byte [EBX+ECX*2+3]
        0x89, 0x43, 4, // MOV [EBX+4],EAX
        0x89, 0xc2, // MOV EDX,EAX
        0xeb, 0,
    ]);
    image.cpu.registers.ebx = 0x8000;
    image.cpu.registers.ecx = 2;
    image.map(8, 0x5000, true);
    image.data(0x5007, &[0xd6]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0xd6;
    cpu.registers.edx = 0xd6;
    cpu.eip = 0x100c;
    cpu.instruction_count = 3;
    check(
        engine,
        module,
        "extended memory fields join primary memory and register successors",
        &image,
        Step {
            cpu,
            ram: &[(0x5004, &[0xd6, 0, 0, 0])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

fn mappings_between_entries(engine: Engine, module: &TestModule) {
    let mut image = Image::new(&[0xb8, 37, 0, 0, 0, 0xb9, 17, 0, 0, 0, 0xeb, 0xf4]);
    image.data(0x5000, &[0xb8, 42, 0, 0, 0, 0xb9, 99, 0, 0, 0, 0xeb, 0xf4]);
    let mut input = image.input();
    input.patches_before_calls = vec![
        CallPatches::default(),
        CallPatches {
            machine: vec![(4, 0x5001_u32.to_le_bytes().to_vec())],
            ..CallPatches::default()
        },
        CallPatches {
            machine: vec![(4, 0_u32.to_le_bytes().to_vec())],
            ..CallPatches::default()
        },
    ];
    let mut first = image.cpu;
    first.registers.eax = 37;
    first.registers.ecx = 17;
    first.instruction_count = 2;
    let mut second = first;
    second.registers.eax = 42;
    second.registers.ecx = 99;
    second.instruction_count = 5;
    let mut observation = expected(
        &image,
        &[
            Step {
                cpu: first,
                ram: &[],
                exit: Exit::Dispatch(0x1000),
            },
            Step {
                cpu: second,
                ram: &[],
                exit: Exit::Dispatch(0x1000),
            },
            Step {
                cpu: second,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x1000,
                    error: 16,
                },
            },
        ],
    );
    observation.machine_unchanged = false;
    assert_eq!(
        engine.observe(module, &input, 3),
        observation,
        "each entry observes host remapping and removal of the previous code page"
    );
}

fn completed_progress_at_faults(engine: Engine, module: &TestModule) {
    for (name, start, suffix, restart, exit) in [
        (
            "absent next opcode",
            0x1ffb,
            &[][..],
            0x2000,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "absent next ModRM",
            0x1ffa,
            &[0x89][..],
            0x1fff,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "incomplete next immediate",
            0x1ff7,
            &[0xb9, 1, 2, 3][..],
            0x1ffc,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "next data access faults",
            0x1000,
            &[0x8b, 0x13][..],
            0x1005,
            Exit::PageFault {
                address: 0x4444_4444,
                error: 0,
            },
        ),
        (
            "next opcode is unsupported",
            0x1000,
            &[0xf4][..],
            0x1005,
            Exit::Other(0x0008_00f4_0000_1005),
        ),
    ] {
        let mut image = Image::empty();
        image.cpu.eip = start;
        image.map(1, 0x3000, false);
        let mut code = vec![0xb8, 42, 0, 0, 0];
        code.extend_from_slice(suffix);
        image.data(0x3000 + (start & 0xfff), &code);
        let mut cpu = image.cpu;
        cpu.registers.eax = 42;
        cpu.eip = restart;
        cpu.instruction_count = 0;
        check(
            engine,
            module,
            name,
            &image,
            Step {
                cpu,
                ram: &[],
                exit,
            },
        );
    }
}

fn live_bytes_and_prefixes(engine: Engine, module: &TestModule) {
    let mut image = Image::new(&[
        0xc6, 0x05, 8, 0x10, 0, 0, 42, // MOV byte [1008],42
        0xb0, 7, // MOV AL,7; preceding store replaces its immediate
        0xeb, 0,
    ]);
    image.map(1, 0x3000, true);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1111_112a;
    cpu.eip = 0x100b;
    cpu.instruction_count = 2;
    check(
        engine,
        module,
        "a completed store changes the next instruction's live bytes",
        &image,
        Step {
            cpu,
            ram: &[(0x3008, &[42])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );

    let mut image = Image::new(&[
        0x66, 0xb8, 0x34, 0x12, // MOV AX,1234
        0xb9, 0x78, 0x56, 0x34, 0x12, // MOV ECX,12345678
        0x64, 0x8b, 0x13, // MOV EDX,FS:[EBX]
        0x8b, 0x03, // MOV EAX,[EBX]
        0xeb, 0,
    ]);
    image.cpu.registers.ebx = 0x8000;
    image.cpu.segments.fs.base = 0x1000;
    image.map(8, 0x5000, false);
    image.map(9, 0x6000, false);
    image.data(0x5000, &[0x11; 4]);
    image.data(0x6000, &[0x22; 4]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1111_1111;
    cpu.registers.ecx = 0x1234_5678;
    cpu.registers.edx = 0x2222_2222;
    cpu.eip = 0x1010;
    cpu.instruction_count = 4;
    check(
        engine,
        module,
        "operand and segment prefixes end with their instruction",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}
