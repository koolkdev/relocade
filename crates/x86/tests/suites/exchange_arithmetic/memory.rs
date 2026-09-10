use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::image;

#[test]
fn write_access_faults_preserve_entry_state_even_when_comparison_would_fail() {
    struct Case {
        name: &'static str,
        width: u8,
        address: u32,
        first_writable: Option<bool>,
        second_writable: Option<bool>,
        fault_address: u32,
        error: u16,
    }
    for case in [
        Case {
            name: "missing byte page",
            width: 8,
            address: 0x4020,
            first_writable: None,
            second_writable: None,
            fault_address: 0x4020,
            error: 2,
        },
        Case {
            name: "read-only unequal dword",
            width: 32,
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Case {
            name: "read-only second word page",
            width: 16,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "missing second dword page",
            width: 32,
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0x5000,
            error: 2,
        },
        Case {
            name: "word range cannot wrap",
            width: 16,
            address: 0xffff_ffff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_ffff,
            error: 2,
        },
        Case {
            name: "dword range cannot wrap",
            width: 32,
            address: 0xffff_fffd,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_fffd,
            error: 2,
        },
    ] {
        for (operation, opcode) in [("XADD", 0xc0), ("CMPXCHG", 0xb0)] {
            let mut code = Vec::new();
            if case.width == 16 {
                code.push(0x66);
            }
            code.extend_from_slice(&[0x0f, opcode + u8::from(case.width != 8), 0x03]);
            let mut image = image(&code);
            image.cpu.registers.ebx = case.address;
            // A pending arithmetic source must survive as well as concrete status bytes.
            image.cpu.flags.kind = 9;
            image.cpu.flags.left = 0x7fff_fffe;
            image.cpu.flags.right = 0xffff_fffe;
            if let Some(writable) = case.first_writable {
                image.map(case.address >> 12, 0x8000, writable);
            }
            if let Some(writable) = case.second_writable {
                image.map(5, 0xa000, writable);
            }
            image.data(0x801f, &[0x5a, 0x80, 0xfe, 0xdc, 0xba, 0x5a]);
            image.data(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]);
            image.data(0xa000, &[0x12, 0x5a]);
            both(
                TestModule::interpreter(),
                &format!("{operation}: {}", case.name),
                &code,
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
}

#[test]
fn dword_addition_crosses_scattered_pages_without_extra_writes() {
    let code = [0x0f, 0xc1, 0x03]; // XADD [EBX],EAX
    let mut image = image(&code);
    image.cpu.registers.eax = 1;
    image.cpu.registers.ebx = 0x4ffe;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, true);
    image.data(0x8ffd, &[0x5a, 0xff, 0xff]);
    image.data(0xa000, &[0xff, 0xff, 0x5a]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0xffff_ffff;
    cpu.flags.kind = 10;
    cpu.flags.left = 0xffff_ffff;
    cpu.flags.right = 1;
    cpu.eip = 0x1003;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "cross-page XADD reads the old dword and writes its sum",
        &code,
        1,
        &image,
        &[Step {
            cpu,
            ram: &[(0x8ffe, &[0, 0]), (0xa000, &[0, 0])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}

#[test]
fn successful_word_comparison_updates_only_its_two_scattered_bytes() {
    let code = [0x66, 0x0f, 0xb1, 0x0b]; // CMPXCHG [EBX],CX
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4433_ff80;
    image.cpu.registers.ebx = 0x4fff;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, true);
    image.data(0x8ffe, &[0x5a, 0x80]);
    image.data(0xa000, &[0xff, 0x5a]);
    let mut cpu = image.cpu;
    cpu.flags.kind = 5;
    cpu.flags.left = 0xff80;
    cpu.flags.right = 0xff80;
    cpu.eip = 0x1004;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "cross-page CMPXCHG retains its equal accumulator and stores old CX",
        &code,
        1,
        &image,
        &[Step {
            cpu,
            ram: &[(0x8fff, &[0x55]), (0xa000, &[0x66])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
