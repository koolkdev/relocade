use wasm86_x86::Gpr32;

use crate::support::{
    conditions::check_conditions,
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::image;

#[test]
fn exchanged_addition_results_produce_width_specific_flags() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        eax: u32,
        ebx: u32,
        result_eax: u32,
        result_ebx: u32,
        kind: u8,
        left: u32,
        right: u32,
        conditions: u16,
    }
    for case in [
        Case {
            name: "byte carry and signed overflow",
            code: &[0x0f, 0xc0, 0xd8],
            eax: 0x4433_2280,
            ebx: 0x10ff_ee80,
            result_eax: 0x4433_2200,
            result_ebx: 0x10ff_ee80,
            kind: 2,
            left: 0x80,
            right: 0x80,
            conditions: 0x5655,
        },
        Case {
            name: "byte auxiliary carry with odd low-byte parity",
            code: &[0x0f, 0xc0, 0xd8],
            eax: 0x4433_220f,
            ebx: 0x10ff_ee01,
            result_eax: 0x4433_2210,
            result_ebx: 0x10ff_ee0f,
            kind: 2,
            left: 0x0f,
            right: 1,
            conditions: 0xaaaa,
        },
        Case {
            name: "word signed overflow with even low-byte parity",
            code: &[0x66, 0x0f, 0xc1, 0xd8],
            eax: 0x4433_7fff,
            ebx: 0x10ff_0001,
            result_eax: 0x4433_8000,
            result_ebx: 0x10ff_7fff,
            kind: 6,
            left: 0x7fff,
            right: 1,
            conditions: 0xa5a9,
        },
        Case {
            name: "dword carry and zero without signed overflow",
            code: &[0x0f, 0xc1, 0xd8],
            eax: 0xffff_ffff,
            ebx: 1,
            result_eax: 0,
            result_ebx: 0xffff_ffff,
            kind: 10,
            left: 0xffff_ffff,
            right: 1,
            conditions: 0x6656,
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.eax = case.eax;
        image.cpu.registers.ebx = case.ebx;
        let mut cpu = image.cpu;
        cpu.registers.eax = case.result_eax;
        cpu.registers.ebx = case.result_ebx;
        cpu.flags.kind = case.kind;
        cpu.flags.left = case.left;
        cpu.flags.right = case.right;
        check_conditions(
            TestModule::interpreter(),
            case.name,
            case.code,
            &mut image,
            &cpu,
            case.conditions,
        );
    }
}

#[test]
fn aliases_read_old_values_and_the_destination_write_wins() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        initial: &'static [(Gpr32, u32)],
        changes: &'static [(Gpr32, u32)],
        kind: u8,
        left: u32,
        right: u32,
    }
    for case in [
        Case {
            name: "AL and AH with an ignored operand-size override",
            code: &[0x66, 0x0f, 0xc0, 0xe0],
            initial: &[],
            changes: &[(Gpr32::Eax, 0x4433_1133)],
            kind: 2,
            left: 0x11,
            right: 0x22,
        },
        Case {
            name: "AH destination and AL source share their parent",
            code: &[0x0f, 0xc0, 0xc4],
            initial: &[],
            changes: &[(Gpr32::Eax, 0x4433_3322)],
            kind: 2,
            left: 0x22,
            right: 0x11,
        },
        Case {
            name: "same high-byte register doubles",
            code: &[0x0f, 0xc0, 0xe4],
            initial: &[],
            changes: &[(Gpr32::Eax, 0x4433_4411)],
            kind: 2,
            left: 0x22,
            right: 0x22,
        },
        Case {
            name: "same word register doubles and keeps its upper half",
            code: &[0x66, 0x0f, 0xc1, 0xc0],
            initial: &[],
            changes: &[(Gpr32::Eax, 0x4433_4422)],
            kind: 6,
            left: 0x2211,
            right: 0x2211,
        },
        Case {
            name: "same dword register doubles through carry and overflow",
            code: &[0x0f, 0xc1, 0xc0],
            initial: &[(Gpr32::Eax, 0x8000_0000)],
            changes: &[(Gpr32::Eax, 0)],
            kind: 10,
            left: 0x8000_0000,
            right: 0x8000_0000,
        },
        Case {
            name: "ESI receives the sum while EBP receives old ESI",
            code: &[0x0f, 0xc1, 0xee],
            initial: &[],
            changes: &[(Gpr32::Esi, 0xdddd_dddd), (Gpr32::Ebp, 0x7777_7777)],
            kind: 10,
            left: 0x7777_7777,
            right: 0x6666_6666,
        },
        Case {
            name: "SP and DI keep both upper halves",
            code: &[0x66, 0x0f, 0xc1, 0xfc],
            initial: &[],
            changes: &[(Gpr32::Esp, 0x5555_dddd), (Gpr32::Edi, 0x8888_5555)],
            kind: 6,
            left: 0x5555,
            right: 0x8888,
        },
    ] {
        let mut image = image(case.code);
        for &(register, value) in case.initial {
            image.cpu.registers[register] = value;
        }
        let mut cpu = image.cpu;
        for &(register, value) in case.changes {
            cpu.registers[register] = value;
        }
        cpu.flags.kind = case.kind;
        cpu.flags.left = case.left;
        cpu.flags.right = case.right;
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
fn memory_additions_keep_the_old_base_and_scaled_index() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        register: Gpr32,
        old: u32,
        result: u32,
        memory: &'static [u8],
        written: &'static [u8],
        kind: u8,
        left: u32,
        right: u32,
    }
    for case in [
        Case {
            name: "dword source is the address base",
            code: &[0x0f, 0xc1, 0x00],
            register: Gpr32::Eax,
            old: 0x4010,
            result: 1,
            memory: &[1, 0, 0, 0],
            written: &[0x11, 0x40, 0, 0],
            kind: 10,
            left: 1,
            right: 0x4010,
        },
        Case {
            name: "AH source shares its address base",
            code: &[0x0f, 0xc0, 0x20],
            register: Gpr32::Eax,
            old: 0x4010,
            result: 0x8010,
            memory: &[0x80],
            written: &[0xc0],
            kind: 2,
            left: 0x80,
            right: 0x40,
        },
        Case {
            name: "word source is a wrapping scaled index",
            code: &[0x66, 0x0f, 0xc1, 0x4c, 0x8b, 0x10],
            register: Gpr32::Ecx,
            old: 0x8000_0004,
            result: 0x8000_ffff,
            memory: &[0xff, 0xff],
            written: &[3, 0],
            kind: 6,
            left: 0xffff,
            right: 4,
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.ebx = 0x3ff0;
        image.cpu.registers[case.register] = case.old;
        image.map(4, 0x8000, true);
        image.data(0x800f, &[0x5a; 6]);
        image.data(0x8010, case.memory);
        let mut cpu = image.cpu;
        cpu.registers[case.register] = case.result;
        cpu.flags.kind = case.kind;
        cpu.flags.left = case.left;
        cpu.flags.right = case.right;
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
                ram: &[(0x8010, case.written)],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}
