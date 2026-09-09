use wasm86_x86::compile_block_from_bytes;

use crate::support::{
    machine::{self, both, Exit, Image, Step},
    step::TestModule,
};

#[test]
fn push_faults_prove_the_entire_stack_write_before_changing_esp_or_ram() {
    struct Fault {
        first_page: Option<bool>,
        next_page: Option<bool>,
        address: u32,
        error: u16,
    }
    for code in [
        &[0x50][..],
        &[0xff, 0xf4],
        &[0xff, 0x33],
        &[0x68, 0x78, 0x56, 0x34, 0x12],
        &[0x6a, 0x80],
        &[0x66, 0x50],
        &[0x66, 0xff, 0xf4],
        &[0x66, 0xff, 0x33],
        &[0x66, 0x68, 0x78, 0x56],
        &[0x66, 0x6a, 0x80],
    ] {
        for fault in [
            Fault {
                first_page: None,
                next_page: None,
                address: 0x4fff,
                error: 2,
            },
            Fault {
                first_page: Some(false),
                next_page: None,
                address: 0x4fff,
                error: 3,
            },
            Fault {
                first_page: Some(true),
                next_page: None,
                address: 0x5000,
                error: 2,
            },
            Fault {
                first_page: Some(true),
                next_page: Some(false),
                address: 0x5000,
                error: 3,
            },
        ] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            image.cpu.registers.esp = if code[0] == 0x66 { 0x5001 } else { 0x5003 };
            image.cpu.registers.ebx = 0x6000;
            image.map(6, 0xc000, false);
            image.data(0xc000, &[0x78, 0x56, 0x34, 0x12]);
            if let Some(writable) = fault.first_page {
                image.map(4, 0x8000, writable);
            }
            if let Some(writable) = fault.next_page {
                image.map(5, 0xa000, writable);
            }
            image.data(0x8ffe, &[0xa5, 0x11]);
            image.data(0xa000, &[0x22, 0x33, 0x44, 0x5a]);
            both(
                TestModule::interpreter(),
                "PUSH stack write fault leaves the instruction uncommitted",
                code,
                1,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: fault.address,
                        error: fault.error,
                    },
                }],
            );
        }
    }
}

#[test]
fn stack_transfers_check_the_complete_read_before_committing_a_destination() {
    for (code, push) in [
        (&[0xff, 0x33][..], true),
        (&[0x66, 0xff, 0x33][..], true),
        (&[0x58][..], false),
        (&[0x66, 0x58][..], false),
        (&[0x5c][..], false),
        (&[0x66, 0x5c][..], false),
        (&[0x8f, 0x03][..], false),
        (&[0x66, 0x8f, 0x03][..], false),
    ] {
        for (first_present, fault_address) in [(false, 0x4fff), (true, 0x5000)] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            image.cpu.registers.esp = if push { 0x9004 } else { 0x4fff };
            image.cpu.registers.ebx = if push { 0x4fff } else { 0x9000 };
            if first_present {
                image.map(4, 0x8000, false);
            }
            image.map(9, 0xb000, true);
            image.data(0x8ffe, &[0xa5, 0x11]);
            image.data(0xa000, &[0x22, 0x33, 0x44, 0x5a]);
            image.data(0xb000, &[0x5a; 8]);
            both(
                TestModule::interpreter(),
                "a missing source byte leaves all stack-transfer state unchanged",
                code,
                1,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: fault_address,
                        error: 0,
                    },
                }],
            );
        }
    }
}

#[test]
fn pop_destination_faults_leave_the_original_stack_pointer_and_memory() {
    struct Fault {
        first_page: Option<bool>,
        next_page: Option<bool>,
        address: u32,
        error: u16,
    }
    for code in [&[0x8f, 0x03][..], &[0x66, 0x8f, 0x03]] {
        for fault in [
            Fault {
                first_page: None,
                next_page: None,
                address: 0x4fff,
                error: 2,
            },
            Fault {
                first_page: Some(false),
                next_page: None,
                address: 0x4fff,
                error: 3,
            },
            Fault {
                first_page: Some(true),
                next_page: None,
                address: 0x5000,
                error: 2,
            },
            Fault {
                first_page: Some(true),
                next_page: Some(false),
                address: 0x5000,
                error: 3,
            },
        ] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            image.cpu.registers.esp = 0x9000;
            image.cpu.registers.ebx = 0x4fff;
            image.map(9, 0xb000, false);
            if let Some(writable) = fault.first_page {
                image.map(4, 0x8000, writable);
            }
            if let Some(writable) = fault.next_page {
                image.map(5, 0xa000, writable);
            }
            image.data(0xb000, &[0x78, 0x56, 0x34, 0x12]);
            image.data(0x8ffe, &[0xa5, 0x11]);
            image.data(0xa000, &[0x22, 0x33, 0x44, 0x5a]);
            both(
                TestModule::interpreter(),
                "POP destination is fully guarded before ESP is defined",
                code,
                1,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: fault.address,
                        error: fault.error,
                    },
                }],
            );
        }
    }
}

#[test]
fn pop_esp_based_destination_faults_do_not_publish_the_prospective_esp() {
    for (code, stack_pointer) in [
        (&[0x8f, 0x04, 0x24][..], 0x4ffc),
        (&[0x66, 0x8f, 0x04, 0x24][..], 0x4ffe),
        (&[0x8f, 0x44, 0x8c, 0xf4][..], 0x4ffc),
        (&[0x66, 0x8f, 0x44, 0x8c, 0xf4][..], 0x4ffe),
    ] {
        for (present, error) in [(false, 2), (true, 3)] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            image.cpu.registers.esp = stack_pointer;
            image.cpu.registers.ecx = 3;
            image.map(4, 0x8000, false);
            if present {
                image.map(5, 0xa000, false);
            }
            image.data(0x8ffc, &[0x78, 0x56, 0x34, 0x12]);
            image.data(0xa000, &[0x5a; 4]);
            both(
                TestModule::interpreter(),
                "POP postincrement address faults with preinstruction ESP",
                code,
                1,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error,
                    },
                }],
            );
        }
    }
}

#[test]
fn stack_transfers_report_the_source_fault_before_a_destination_fault() {
    for (code, push) in [
        (&[0xff, 0x33][..], true),
        (&[0x66, 0xff, 0x33][..], true),
        (&[0x8f, 0x03][..], false),
        (&[0x66, 0x8f, 0x03][..], false),
    ] {
        let mut image = Image::new(code);
        image.cpu.flags.kind = 0xff;
        image.cpu.registers.esp = if push { 0x5003 } else { 0x4fff };
        image.cpu.registers.ebx = 0x6fff;
        both(
            TestModule::interpreter(),
            "shared stack semantics check the read before the write",
            code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: if push { 0x6fff } else { 0x4fff },
                    error: 0,
                },
            }],
        );
    }
}

#[test]
fn stack_memory_spans_follow_the_nonwrapping_data_access_policy() {
    for (code, stack_pointer, address, error) in [
        (&[0x50][..], 2, 0xffff_fffe, 2),
        (&[0x66, 0x50][..], 1, 0xffff_ffff, 2),
        (&[0x58][..], 0xffff_fffe, 0xffff_fffe, 0),
        (&[0x66, 0x58][..], 0xffff_ffff, 0xffff_ffff, 0),
    ] {
        let mut image = Image::new(code);
        image.cpu.flags.kind = 0xff;
        image.cpu.registers.esp = stack_pointer;
        image.map(0xfffff, 0x8000, true);
        image.map(0, 0xa000, true);
        image.data(0x8ffe, &[0x11, 0x22]);
        image.data(0xa000, &[0x33, 0x44]);
        both(
            TestModule::interpreter(),
            "stack pointer wrap does not permit an operand span to wrap",
            code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault { address, error },
            }],
        );
    }
}

struct StackFaultScenario {
    code: Vec<u8>,
    image: Image,
    steps: Vec<Step<'static>>,
}

fn completed_stack_transfers_before_fault() -> StackFaultScenario {
    let code = vec![
        0x50, // PUSH EAX across two stack pages.
        0x66, 0x8f, 0x04, 0x24, // POP word [ESP] at the incremented address.
        0x5a, // POP EDX from the word just written and its adjacent bytes.
        0x8f, 0x01, // POP dword [ECX], whose destination page is absent.
    ];
    let mut image = Image::new(&code);
    image.cpu.flags.kind = 0xff;
    image.cpu.registers.eax = 0x1234_5678;
    image.cpu.registers.esp = 0x5002;
    image.cpu.registers.ecx = 0x6000;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, true);
    image.data(0x8ffd, &[0xa5; 3]);
    image.data(
        0xa000,
        &[0xa5, 0xa5, 0xbe, 0xad, 0x11, 0x22, 0x33, 0x44, 0x5a],
    );
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.esp = 0x4ffe;
    expected_cpu.eip = 0x1001;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8ffe, &[0x78, 0x56]), (0xa000, &[0x34, 0x12])],
        exit: Exit::Dispatch(0x1001),
    });

    expected_cpu.registers.esp = 0x5000;
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0xa000, &[0x78, 0x56])],
        exit: Exit::Dispatch(0x1005),
    });

    expected_cpu.registers.edx = 0xadbe_5678;
    expected_cpu.registers.esp = 0x5004;
    expected_cpu.eip = 0x1006;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1006),
    });

    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x6000,
            error: 2,
        },
    });

    StackFaultScenario { code, image, steps }
}

#[test]
fn completed_push_and_pop_effects_survive_a_later_pop_destination_fault() {
    let StackFaultScenario { code, image, steps } = completed_stack_transfers_before_fault();
    both(
        TestModule::interpreter(),
        "completed stack effects precede a fault",
        &code,
        4,
        &image,
        &steps,
    );
}

#[derive(Clone, Copy, Debug)]
enum PopAddressCase {
    SourceMissing,
    DestinationMissing,
    DestinationReadOnly,
    Success,
}

const POP_ADDRESS_CASES: [PopAddressCase; 4] = [
    PopAddressCase::SourceMissing,
    PopAddressCase::DestinationMissing,
    PopAddressCase::DestinationReadOnly,
    PopAddressCase::Success,
];

fn completed_arithmetic_before_pop(case: PopAddressCase) -> StackFaultScenario {
    let code = vec![
        0x01, 0xd0, // ADD EAX, EDX leaves a completed register and flag definition.
        0xbc, 0xfe, 0x4f, 0, 0, // MOV ESP, 0x4ffe replaces the entry stack pointer.
        0x66, 0x8f, 0x44, 0xb4, 0xf4, // POP word [ESP + ESI*4 - 12].
        0x89, 0xe7, // MOV EDI, ESP observes the successful increment.
    ];
    let mut image = Image::new(&code);
    image.cpu.flags.kind = 0xff;
    image.cpu.registers.eax = 0x7fff_fffe;
    image.cpu.registers.edx = 2;
    image.cpu.registers.esp = 0x9000;
    image.cpu.registers.esi = 3;
    if !matches!(case, PopAddressCase::SourceMissing) {
        image.map(4, 0x8000, false);
    }
    match case {
        PopAddressCase::SourceMissing | PopAddressCase::Success => image.map(5, 0xa000, true),
        PopAddressCase::DestinationReadOnly => image.map(5, 0xa000, false),
        PopAddressCase::DestinationMissing => {}
    }
    image.data(0x8ffd, &[0x5a, 0x78, 0x56]);
    image.data(0xa000, &[0xa5; 4]);

    let mut cpu = image.cpu;
    cpu.registers.eax = 0x8000_0000;
    cpu.flags.kind = 10;
    cpu.flags.left = 0x7fff_fffe;
    cpu.flags.right = 2;
    cpu.eip = 0x1002;
    cpu.instruction_count = 0;
    let mut steps = vec![Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1002),
    }];

    cpu.registers.esp = 0x4ffe;
    cpu.eip = 0x1007;
    cpu.instruction_count = 1;
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1007),
    });

    let fault = match case {
        PopAddressCase::SourceMissing => Some((0x4ffe, 0)),
        PopAddressCase::DestinationMissing => Some((0x5000, 2)),
        PopAddressCase::DestinationReadOnly => Some((0x5000, 3)),
        PopAddressCase::Success => None,
    };
    if let Some((address, error)) = fault {
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::PageFault { address, error },
        });
    } else {
        cpu.registers.esp = 0x5000;
        cpu.eip = 0x100c;
        cpu.instruction_count = 2;
        steps.push(Step {
            cpu,
            ram: &[(0xa000, &[0x78, 0x56])],
            exit: Exit::Dispatch(0x100c),
        });
        cpu.registers.edi = 0x5000;
        cpu.eip = 0x100e;
        cpu.instruction_count = 3;
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(0x100e),
        });
    }
    StackFaultScenario { code, image, steps }
}

#[test]
fn pop_address_guards_preserve_completed_arithmetic_and_stack_pointer_definitions() {
    for case in POP_ADDRESS_CASES {
        let StackFaultScenario { code, image, steps } = completed_arithmetic_before_pop(case);
        both(
            TestModule::interpreter(),
            &format!("completed arithmetic before POP address guards: {case:?}"),
            &code,
            4,
            &image,
            &steps,
        );
    }
}

fn check_stack_scenario_in_v8(name: &str, scenario: StackFaultScenario) {
    let StackFaultScenario { code, image, steps } = scenario;
    let last = steps.last().unwrap();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        machine::expected(&image, &steps),
        "{name}: interpreter",
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap());
    let ram = steps
        .iter()
        .flat_map(|step| step.ram.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: last.cpu,
                ram: &ram,
                exit: last.exit,
            }]
        ),
        "{name}: snapshot",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn stack_aliases_and_fault_publication_execute_in_optimizing_v8() {
    check_stack_scenario_in_v8(
        "stack aliases and completed memory writes",
        completed_stack_transfers_before_fault(),
    );
    for case in POP_ADDRESS_CASES {
        check_stack_scenario_in_v8(
            &format!("completed arithmetic before POP address guards: {case:?}"),
            completed_arithmetic_before_pop(case),
        );
    }
}
