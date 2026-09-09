use crate::support::{
    guest::{Exit, Machine, Permissions},
    machine::{both, Image, Step},
    step::TestModule,
};

#[test]
fn push_memory_reads_the_source_using_esp_before_the_decrement() {
    for (code, address, bytes) in [
        (
            &[0xff, 0x34, 0x24][..],
            0x5000,
            &[0x55, 0x66, 0x77, 0x88][..],
        ),
        (
            &[0xff, 0x74, 0x24, 0xfe][..],
            0x5000,
            &[0x33, 0x44, 0x55, 0x66],
        ),
        (
            &[0xff, 0x74, 0x8c, 0xf4][..],
            0x5000,
            &[0x55, 0x66, 0x77, 0x88],
        ),
        (&[0x66, 0xff, 0x34, 0x24][..], 0x5002, &[0x55, 0x66]),
        (&[0x66, 0xff, 0x74, 0x24, 0xff][..], 0x5002, &[0x44, 0x55]),
        (&[0x66, 0xff, 0x74, 0x8c, 0xf4][..], 0x5002, &[0x55, 0x66]),
    ] {
        let mut machine = Machine::new(code);
        machine.cpu.flags.kind = 0xff;
        machine.cpu.registers.esp = 0x5004;
        machine.cpu.registers.ecx = 3;
        machine.memory(
            0x5000,
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99],
            Permissions::ReadWrite,
        );
        let mut expected = machine.state();
        expected.cpu.registers.esp = address;
        expected.cpu.eip = 0x1000 + code.len() as u32;
        expected.cpu.instruction_count = 0;
        expected.memory.write(address, bytes);
        for execution in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
            assert_eq!(execution.state, expected, "{code:02x?}");
            assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
            assert!(execution.machine_unchanged);
        }
    }
}

#[test]
fn pop_memory_uses_incremented_esp_only_for_present_address_components() {
    struct Case {
        code: &'static [u8],
        destination: u32,
        stack_pointer: u32,
        bytes: &'static [u8],
    }
    for case in [
        Case {
            code: &[0x8f, 0x04, 0x24],
            destination: 0x5004,
            stack_pointer: 0x5004,
            bytes: &[0x11, 0x22, 0x33, 0x44],
        },
        Case {
            code: &[0x8f, 0x44, 0x24, 0xfc],
            destination: 0x5000,
            stack_pointer: 0x5004,
            bytes: &[0x11, 0x22, 0x33, 0x44],
        },
        Case {
            code: &[0x8f, 0x44, 0x24, 0xfe],
            destination: 0x5002,
            stack_pointer: 0x5004,
            bytes: &[0x11, 0x22, 0x33, 0x44],
        },
        Case {
            code: &[0x8f, 0x44, 0x8c, 0xf4],
            destination: 0x5004,
            stack_pointer: 0x5004,
            bytes: &[0x11, 0x22, 0x33, 0x44],
        },
        Case {
            code: &[0x8f, 0x03],
            destination: 0x6000,
            stack_pointer: 0x5004,
            bytes: &[0x11, 0x22, 0x33, 0x44],
        },
        Case {
            code: &[0x8f, 0x04, 0xa5, 0, 0x60, 0, 0],
            destination: 0x6000,
            stack_pointer: 0x5004,
            bytes: &[0x11, 0x22, 0x33, 0x44],
        },
        Case {
            code: &[0x66, 0x8f, 0x04, 0x24],
            destination: 0x5002,
            stack_pointer: 0x5002,
            bytes: &[0x11, 0x22],
        },
        Case {
            code: &[0x66, 0x8f, 0x44, 0x24, 0xfe],
            destination: 0x5000,
            stack_pointer: 0x5002,
            bytes: &[0x11, 0x22],
        },
        Case {
            code: &[0x66, 0x8f, 0x44, 0x8c, 0xf4],
            destination: 0x5002,
            stack_pointer: 0x5002,
            bytes: &[0x11, 0x22],
        },
        Case {
            code: &[0x66, 0x8f, 0x03],
            destination: 0x6000,
            stack_pointer: 0x5002,
            bytes: &[0x11, 0x22],
        },
        Case {
            code: &[0x66, 0x8f, 0x04, 0xa5, 0, 0x60, 0, 0],
            destination: 0x6000,
            stack_pointer: 0x5002,
            bytes: &[0x11, 0x22],
        },
    ] {
        let mut machine = Machine::new(case.code);
        machine.cpu.flags.kind = 0xff;
        machine.cpu.registers.esp = 0x5000;
        machine.cpu.registers.ecx = 3;
        machine.cpu.registers.ebx = 0x6000;
        machine.memory(
            0x5000,
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99],
            Permissions::ReadWrite,
        );
        machine.memory(0x6000, &[0xa5; 8], Permissions::ReadWrite);
        let mut expected = machine.state();
        expected.cpu.registers.esp = case.stack_pointer;
        expected.cpu.eip = 0x1000 + case.code.len() as u32;
        expected.cpu.instruction_count = 0;
        expected.memory.write(case.destination, case.bytes);
        for execution in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
            assert_eq!(execution.state, expected, "{:02x?}", case.code);
            assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
            assert!(execution.machine_unchanged);
        }
    }
}

#[test]
fn push_memory_resolves_split_source_and_stack_ranges_independently() {
    for (code, stack_pointer, width) in [
        (&[0xff, 0x33][..], 0x5003, 4),
        (&[0x66, 0xff, 0x33][..], 0x5001, 2),
    ] {
        for (stack_frame, source_frame) in [(0x9000, 0xc000), (0xa000, 0xe000)] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            image.cpu.registers.esp = stack_pointer;
            image.cpu.registers.ebx = 0x6fff;
            image.map(4, 0x8000, true);
            image.map(5, stack_frame, true);
            image.map(6, 0xb000, false);
            image.map(7, source_frame, false);
            image.data(0x8ffe, &[0xa5, 0xa5]);
            image.data(stack_frame, &[0xa5; 4]);
            image.data(0xbffe, &[0x5a, 0x78]);
            image.data(source_frame, &[0x56, 0x34, 0x12, 0x5a]);
            let mut expected_cpu = image.cpu;
            expected_cpu.registers.esp = 0x4fff;
            expected_cpu.eip = 0x1000 + code.len() as u32;
            expected_cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "PUSH reads and writes complete split operands",
                code,
                1,
                &image,
                &[Step {
                    cpu: expected_cpu,
                    ram: &[
                        (0x8fff, &[0x78]),
                        (stack_frame, &[0x56, 0x34, 0x12][..width - 1]),
                    ],
                    exit: Exit::Dispatch(expected_cpu.eip),
                }],
            );
        }
    }
}

#[test]
fn pop_memory_resolves_split_stack_and_destination_ranges_independently() {
    for (code, stack_pointer, width) in [
        (&[0x8f, 0x03][..], 0x5003, 4),
        (&[0x66, 0x8f, 0x03][..], 0x5001, 2),
    ] {
        for (stack_frame, destination_frame) in [(0x9000, 0xc000), (0xa000, 0xe000)] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            image.cpu.registers.esp = 0x4fff;
            image.cpu.registers.ebx = 0x6fff;
            image.map(4, 0x8000, false);
            image.map(5, stack_frame, false);
            image.map(6, 0xb000, true);
            image.map(7, destination_frame, true);
            image.data(0x8ffe, &[0x5a, 0x78]);
            image.data(stack_frame, &[0x56, 0x34, 0x12, 0x5a]);
            image.data(0xbffe, &[0xa5, 0xa5]);
            image.data(destination_frame, &[0xa5; 4]);
            let mut expected_cpu = image.cpu;
            expected_cpu.registers.esp = stack_pointer;
            expected_cpu.eip = 0x1000 + code.len() as u32;
            expected_cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                "POP reads and writes complete split operands",
                code,
                1,
                &image,
                &[Step {
                    cpu: expected_cpu,
                    ram: &[
                        (0xbfff, &[0x78]),
                        (destination_frame, &[0x56, 0x34, 0x12][..width - 1]),
                    ],
                    exit: Exit::Dispatch(expected_cpu.eip),
                }],
            );
        }
    }
}
