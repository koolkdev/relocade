use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    guest::{Exit, Machine},
    machine::{check, Image, Step},
    step::TestModule,
};

#[test]
fn increment_and_decrement_read_carry_from_stored_sources() {
    struct Source {
        kind: u8,
        left: u32,
        right: u32,
        stored_carry: u8,
        carry: u8,
    }
    for source in [
        Source {
            kind: 0,
            left: 0xffff_ffff,
            right: 1,
            stored_carry: 0x80,
            carry: 0,
        },
        Source {
            kind: 0,
            left: 0,
            right: 1,
            stored_carry: 0xff,
            carry: 1,
        },
        Source {
            kind: 2,
            left: 0xff,
            right: 1,
            stored_carry: 0,
            carry: 1,
        },
        Source {
            kind: 5,
            left: 0,
            right: 1,
            stored_carry: 0,
            carry: 1,
        },
        Source {
            kind: 9,
            left: 3,
            right: 1,
            stored_carry: 1,
            carry: 0,
        },
        Source {
            kind: 11,
            left: 0x8000_0000,
            right: 0xffff_ffff,
            stored_carry: 1,
            carry: 0,
        },
    ] {
        for (code, input, result, sign) in [
            (&[0x40][..], 0x7fff_ffff, 0x8000_0000, 1),
            (&[0x48][..], 0x8000_0000, 0x7fff_ffff, 0),
        ] {
            let mut machine = Machine::new(code);
            machine.cpu.flags.kind = source.kind;
            machine.cpu.flags.left = source.left;
            machine.cpu.flags.right = source.right;
            machine.cpu.flags.status.cf = source.stored_carry;
            machine.cpu.registers.eax = input;
            let mut expected = machine.state();
            expected.cpu.registers.eax = result;
            expected.cpu.flags.kind = 0;
            expected.cpu.flags.status = StatusFlags {
                cf: source.carry,
                pf: 1,
                af: 1,
                zf: 0,
                sf: sign,
                of: 1,
            };
            expected.cpu.eip = 0x1001;
            expected.cpu.instruction_count = 0;
            for execution in [machine.run_step(), machine.run_block(1)] {
                assert_eq!(execution.exit, Exit::Dispatch(0x1001));
                assert_eq!(
                    execution.state, expected,
                    "{code:02x?}, source kind {}",
                    source.kind
                );
                assert_eq!(execution.dispatches, [(0x1001, expected.clone())]);
                assert!(execution.machine_unchanged);
            }
        }
    }
}

#[test]
fn invalid_carry_records_trap_before_a_register_update() {
    for code in [
        &[0xfe, 0xc4][..],
        &[0x66, 0x4b],
        &[0xff, 0xc0],
        &[0xff, 0xc8],
    ] {
        let mut machine = Machine::new(code);
        machine.cpu.flags.kind = 0xff;
        let expected = machine.state();
        for execution in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(execution.exit, Exit::Trap);
            assert_eq!(execution.state, expected);
            assert!(execution.dispatches.is_empty());
            assert!(execution.machine_unchanged);
        }
    }
}

#[test]
fn local_carry_survives_increment_and_decrement_then_feeds_adc_sbb_and_setcc() {
    struct Producer {
        name: &'static str,
        code: &'static [u8],
        ecx: u32,
        edx: u32,
        initial_carry: u8,
        result: u32,
        kind: u8,
        left: u32,
        right: u32,
        status: Option<StatusFlags>,
        carry: u8,
    }
    for producer in [
        Producer {
            name: "ADD carry",
            code: &[0x01, 0xd1],
            ecx: 0xffff_ffff,
            edx: 1,
            initial_carry: 0,
            result: 0,
            kind: 10,
            left: 0xffff_ffff,
            right: 1,
            status: None,
            carry: 1,
        },
        Producer {
            name: "SUB borrow",
            code: &[0x29, 0xd1],
            ecx: 0,
            edx: 1,
            initial_carry: 0,
            result: 0xffff_ffff,
            kind: 9,
            left: 0,
            right: 1,
            status: None,
            carry: 1,
        },
        Producer {
            name: "XOR clears carry",
            code: &[0x31, 0xc9],
            ecx: 0x1234_5678,
            edx: 0,
            initial_carry: 1,
            result: 0,
            kind: 11,
            left: 0,
            right: 0xa5a5_a5a5,
            status: None,
            carry: 0,
        },
        Producer {
            name: "ADC explicit carry",
            code: &[0x11, 0xd1],
            ecx: 0xffff_ffff,
            edx: 0,
            initial_carry: 1,
            result: 0,
            kind: 0,
            left: 0xa5a5_a5a5,
            right: 0xa5a5_a5a5,
            status: Some(StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            }),
            carry: 1,
        },
    ] {
        let code = [
            producer.code,
            &[
                0x40, // INC EAX
                0x4b, // DEC EBX
                0x83, 0xd6, 0, // ADC ESI, 0
                0x83, 0xdf, 0, // SBB EDI, 0
                0x0f, 0x92, 0xc1, // SETB CL
                0x0f, 0x94, 0xc2, // SETE DL
            ],
        ]
        .concat();
        let mut image = Image::new(&code);
        image.cpu.flags.kind = 0;
        image.cpu.flags.status.cf = producer.initial_carry;
        image.cpu.registers.eax = 0xffff_ffff;
        image.cpu.registers.ebx = 0;
        image.cpu.registers.ecx = producer.ecx;
        image.cpu.registers.edx = producer.edx;
        image.cpu.registers.esi = 0xffff_ffff;
        image.cpu.registers.edi = 0;
        let mut expected_cpu = image.cpu;
        let mut steps = Vec::new();

        expected_cpu.registers.ecx = producer.result;
        expected_cpu.flags.kind = producer.kind;
        expected_cpu.flags.left = producer.left;
        expected_cpu.flags.right = producer.right;
        if let Some(status) = producer.status {
            expected_cpu.flags.status = status;
        }
        expected_cpu.eip = 0x1002;
        expected_cpu.instruction_count = 0;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1002),
        });

        expected_cpu.registers.eax = 0;
        expected_cpu.flags.kind = 0;
        expected_cpu.flags.status = StatusFlags {
            cf: producer.carry,
            pf: 1,
            af: 1,
            zf: 1,
            sf: 0,
            of: 0,
        };
        expected_cpu.eip = 0x1003;
        expected_cpu.instruction_count = 1;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1003),
        });

        expected_cpu.registers.ebx = 0xffff_ffff;
        expected_cpu.flags.status = StatusFlags {
            cf: producer.carry,
            pf: 1,
            af: 1,
            zf: 0,
            sf: 1,
            of: 0,
        };
        expected_cpu.eip = 0x1004;
        expected_cpu.instruction_count = 2;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1004),
        });

        expected_cpu.registers.esi = if producer.carry == 1 { 0 } else { 0xffff_ffff };
        expected_cpu.flags.status = StatusFlags {
            cf: producer.carry,
            pf: 1,
            af: producer.carry,
            zf: producer.carry,
            sf: 1 - producer.carry,
            of: 0,
        };
        expected_cpu.eip = 0x1007;
        expected_cpu.instruction_count = 3;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1007),
        });

        expected_cpu.registers.edi = if producer.carry == 1 { 0xffff_ffff } else { 0 };
        expected_cpu.flags.status = StatusFlags {
            cf: producer.carry,
            pf: 1,
            af: producer.carry,
            zf: 1 - producer.carry,
            sf: producer.carry,
            of: 0,
        };
        expected_cpu.eip = 0x100a;
        expected_cpu.instruction_count = 4;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x100a),
        });

        expected_cpu.registers.ecx = (producer.result & 0xffff_ff00) | u32::from(producer.carry);
        expected_cpu.eip = 0x100d;
        expected_cpu.instruction_count = 5;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x100d),
        });

        expected_cpu.registers.edx = (producer.edx & 0xffff_ff00) | u32::from(1 - producer.carry);
        expected_cpu.eip = 0x1010;
        expected_cpu.instruction_count = 6;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1010),
        });

        check(TestModule::interpreter(), producer.name, &image, &steps);
        // A block publishes only the final explicit source, not the producer's
        // discarded lazy operands. The interpreter already published those operands.
        expected_cpu.flags.left = image.cpu.flags.left;
        expected_cpu.flags.right = image.cpu.flags.right;
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 7).unwrap());
        check(
            &block,
            producer.name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::Dispatch(0x1010),
            }],
        );
    }
}
