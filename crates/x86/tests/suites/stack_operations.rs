use wasm86_x86::Gpr32;

use crate::support::{
    guest::{Exit, Machine, Permissions},
    machine::{both, Image, Step},
    step::TestModule,
};

#[path = "stack_operations/decoding.rs"]
mod decoding;
#[path = "stack_operations/faults.rs"]
mod faults;
#[path = "stack_operations/memory.rs"]
mod memory;

#[test]
fn register_pushes_use_the_selected_width_and_original_esp() {
    for (index, value) in [
        (0, 0x1111_1111_u32),
        (1, 0x2222_2222),
        (2, 0xdead_beef),
        (3, 0x4444_4444),
        (4, 0x0000_9004),
        (5, 0x6666_6666),
        (6, 0x7777_7777),
        (7, 0x8888_8888),
    ] {
        for suffix in [vec![0x50 + index], vec![0xff, 0xf0 | index]] {
            for (prefix, stack_pointer, width) in [(&[][..], 0x9000, 4), (&[0x66][..], 0x9002, 2)] {
                let code = [prefix, &suffix].concat();
                let mut machine = Machine::new(&code);
                machine.cpu.flags.kind = 0xff;
                machine.cpu.registers.esp = 0x9004;
                machine.memory(0x8fff, &[0xa5; 10], Permissions::ReadWrite);
                let mut expected = machine.state();
                expected.cpu.registers.esp = stack_pointer;
                expected.cpu.eip = 0x1000 + code.len() as u32;
                expected.cpu.instruction_count = 0;
                expected
                    .memory
                    .write(stack_pointer, &value.to_le_bytes()[..width]);
                for execution in [machine.run_step(), machine.run_block(1)] {
                    assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
                    assert_eq!(execution.state, expected, "{code:02x?}");
                    assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
                    assert!(execution.machine_unchanged);
                }
            }
        }
    }
}

#[test]
fn register_pops_overwrite_the_selected_width_after_advancing_esp() {
    for (index, register, word_result) in [
        (0, Gpr32::Eax, 0x1111_5678),
        (1, Gpr32::Ecx, 0x2222_5678),
        (2, Gpr32::Edx, 0xdead_5678),
        (3, Gpr32::Ebx, 0x4444_5678),
        (4, Gpr32::Esp, 0x0000_5678),
        (5, Gpr32::Ebp, 0x6666_5678),
        (6, Gpr32::Esi, 0x7777_5678),
        (7, Gpr32::Edi, 0x8888_5678),
    ] {
        for suffix in [vec![0x58 + index], vec![0x8f, 0xc0 | index]] {
            for (prefix, stack_pointer, result) in [
                (&[][..], 0x9004, 0x9abc_5678),
                (&[0x66][..], 0x9002, word_result),
            ] {
                let code = [prefix, &suffix].concat();
                let mut machine = Machine::new(&code);
                machine.cpu.flags.kind = 0xff;
                machine.cpu.registers.esp = 0x9000;
                machine.memory(
                    0x8fff,
                    &[0xa5, 0x78, 0x56, 0xbc, 0x9a, 0x5a],
                    Permissions::ReadOnly,
                );
                let mut expected = machine.state();
                expected.cpu.registers.esp = stack_pointer;
                expected.cpu.registers[register] = result;
                expected.cpu.eip = 0x1000 + code.len() as u32;
                expected.cpu.instruction_count = 0;
                for execution in [machine.run_step(), machine.run_block(1)] {
                    assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
                    assert_eq!(execution.state, expected, "{code:02x?}, {register:?}");
                    assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
                    assert!(execution.machine_unchanged);
                }
            }
        }
    }
}

#[test]
fn push_immediates_extend_the_encoded_byte_to_the_operand_width() {
    for (code, stack_pointer, bytes) in [
        (&[0x68, 0, 0, 0, 0x80][..], 0x9000, &[0, 0, 0, 0x80][..]),
        (&[0x66, 0x68, 0xef, 0xbe][..], 0x9002, &[0xef, 0xbe]),
        (&[0x6a, 0][..], 0x9000, &[0, 0, 0, 0]),
        (&[0x6a, 0x7f][..], 0x9000, &[0x7f, 0, 0, 0]),
        (&[0x6a, 0x80][..], 0x9000, &[0x80, 0xff, 0xff, 0xff]),
        (&[0x6a, 0xff][..], 0x9000, &[0xff, 0xff, 0xff, 0xff]),
        (&[0x66, 0x6a, 0][..], 0x9002, &[0, 0]),
        (&[0x66, 0x6a, 0x7f][..], 0x9002, &[0x7f, 0]),
        (&[0x66, 0x6a, 0x80][..], 0x9002, &[0x80, 0xff]),
        (&[0x66, 0x6a, 0xff][..], 0x9002, &[0xff, 0xff]),
        (&[0x66, 0x66, 0x6a, 0x80][..], 0x9002, &[0x80, 0xff]),
    ] {
        let mut machine = Machine::new(code);
        machine.cpu.flags.kind = 0xff;
        machine.cpu.registers.esp = 0x9004;
        machine.memory(0x8fff, &[0xa5; 10], Permissions::ReadWrite);
        let mut expected = machine.state();
        expected.cpu.registers.esp = stack_pointer;
        expected.cpu.eip = 0x1000 + code.len() as u32;
        expected.cpu.instruction_count = 0;
        expected.memory.write(stack_pointer, bytes);
        for execution in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
            assert_eq!(execution.state, expected, "{code:02x?}");
            assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
            assert!(execution.machine_unchanged);
        }
    }
}

#[test]
fn pop_sp_keeps_the_upper_half_of_the_incremented_32_bit_stack_pointer() {
    for (stack_pointer, result) in [
        (0x1234_fffe, 0x1235_beef),
        (0x1234_ffff, 0x1235_beef),
        (0xffff_fffe, 0x0000_beef),
    ] {
        for code in [&[0x66, 0x5c][..], &[0x66, 0x8f, 0xc4]] {
            let mut machine = Machine::new(code);
            machine.cpu.flags.kind = 0xff;
            machine.cpu.registers.esp = stack_pointer;
            machine.memory(stack_pointer, &[0xef, 0xbe], Permissions::ReadOnly);
            let mut expected = machine.state();
            expected.cpu.registers.esp = result;
            expected.cpu.eip = 0x1000 + code.len() as u32;
            expected.cpu.instruction_count = 0;
            for execution in [machine.run_step(), machine.run_block(1)] {
                assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
                assert_eq!(
                    execution.state, expected,
                    "{code:02x?}, ESP={stack_pointer:#x}"
                );
                assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
                assert!(execution.machine_unchanged);
            }
        }
    }
}

#[test]
fn stack_pointer_arithmetic_wraps_when_the_memory_operand_itself_fits() {
    struct Case {
        code: &'static [u8],
        initial_stack: u32,
        address: u32,
        input: &'static [u8],
        final_stack: u32,
        eax: u32,
        output: &'static [u8],
    }
    for case in [
        Case {
            code: &[0x54],
            initial_stack: 0,
            address: 0xffff_fffc,
            input: &[0xa5; 4],
            final_stack: 0xffff_fffc,
            eax: 0x1111_1111,
            output: &[0, 0, 0, 0],
        },
        Case {
            code: &[0x66, 0x54],
            initial_stack: 0,
            address: 0xffff_fffe,
            input: &[0xa5; 2],
            final_stack: 0xffff_fffe,
            eax: 0x1111_1111,
            output: &[0, 0],
        },
        Case {
            code: &[0x58],
            initial_stack: 0xffff_fffc,
            address: 0xffff_fffc,
            input: &[0xef, 0xbe, 0xad, 0xde],
            final_stack: 0,
            eax: 0xdead_beef,
            output: &[0xef, 0xbe, 0xad, 0xde],
        },
        Case {
            code: &[0x66, 0x58],
            initial_stack: 0xffff_fffe,
            address: 0xffff_fffe,
            input: &[0xef, 0xbe],
            final_stack: 0,
            eax: 0x1111_beef,
            output: &[0xef, 0xbe],
        },
    ] {
        let mut machine = Machine::new(case.code);
        machine.cpu.flags.kind = 0xff;
        machine.cpu.registers.esp = case.initial_stack;
        machine.memory(case.address, case.input, Permissions::ReadWrite);
        let mut expected = machine.state();
        expected.cpu.registers.esp = case.final_stack;
        expected.cpu.registers.eax = case.eax;
        expected.cpu.eip = 0x1000 + case.code.len() as u32;
        expected.cpu.instruction_count = 0;
        expected.memory.write(case.address, case.output);
        for execution in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
            assert_eq!(
                execution.state, expected,
                "{:02x?}, ESP={:#x}",
                case.code, case.initial_stack
            );
            assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
            assert!(execution.machine_unchanged);
        }
    }
}

#[test]
fn mixed_width_stack_transfers_publish_each_completed_instruction() {
    let code = [
        0x50, // PUSH EAX
        0x66, 0x6a, 0x80, // PUSH word -128
        0x66, 0x5a, // POP DX
        0x5b, // POP EBX
    ];
    let mut image = Image::new(&code);
    image.cpu.flags.kind = 0xff;
    image.cpu.registers.eax = 0x1234_5678;
    image.cpu.registers.esp = 0x9004;
    image.map(8, 0x8000, true);
    image.map(9, 0xa000, true);
    image.data(0x8ffd, &[0xa5; 3]);
    image.data(0xa000, &[0xa5; 5]);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.esp = 0x9000;
    expected_cpu.eip = 0x1001;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0xa000, &[0x78, 0x56, 0x34, 0x12])],
        exit: Exit::Dispatch(0x1001),
    });

    expected_cpu.registers.esp = 0x8ffe;
    expected_cpu.eip = 0x1004;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8ffe, &[0x80, 0xff])],
        exit: Exit::Dispatch(0x1004),
    });

    expected_cpu.registers.edx = 0xdead_ff80;
    expected_cpu.registers.esp = 0x9000;
    expected_cpu.eip = 0x1006;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1006),
    });

    expected_cpu.registers.ebx = 0x1234_5678;
    expected_cpu.registers.esp = 0x9004;
    expected_cpu.eip = 0x1007;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1007),
    });

    both(
        TestModule::interpreter(),
        "mixed word and dword transfers preserve the whole flag record",
        &code,
        4,
        &image,
        &steps,
    );
}
