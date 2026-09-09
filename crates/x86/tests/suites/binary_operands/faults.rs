use super::{
    image,
    machine::{both, Exit, Step},
    step::TestModule,
};

#[test]
fn operand_faults_precede_state_changes() {
    let step = TestModule::interpreter();
    for (name, faulting, first_writable, second_page, fault) in [
        (
            "readonly RMW preserves earlier flags",
            &[0x01, 0x03][..],
            false,
            None,
            Exit::PageFault {
                address: 0x00004ffe,
                error: 0x3,
            },
        ),
        (
            "missing RMW tail keeps every byte",
            &[0x01, 0x03][..],
            true,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x2,
            },
        ),
        (
            "readonly RMW tail keeps every byte",
            &[0x01, 0x03][..],
            true,
            Some(false),
            Exit::PageFault {
                address: 0x00005000,
                error: 0x3,
            },
        ),
        (
            "ADD source fault preserves earlier flags and destination",
            &[0x03, 0x03][..],
            false,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "SUB denied destination preserves earlier flags",
            &[0x29, 0x03][..],
            false,
            None,
            Exit::PageFault {
                address: 0x00004ffe,
                error: 0x3,
            },
        ),
        (
            "AND missing tail prevents even a constant-zero store",
            &[0x81, 0x23, 0, 0, 0, 0][..],
            true,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x2,
            },
        ),
        (
            "OR readonly tail keeps every byte",
            &[0x09, 0x03][..],
            true,
            Some(false),
            Exit::PageFault {
                address: 0x00005000,
                error: 0x3,
            },
        ),
        (
            "XOR source fault preserves earlier flags and destination",
            &[0x33, 0x03][..],
            false,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "TEST missing tail is a read fault",
            &[0x85, 0x03][..],
            false,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "TEST zero immediate still checks the complete read span",
            &[0xf7, 0x03, 0, 0, 0, 0][..],
            false,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "CMP missing tail is a read fault",
            &[0x39, 0x03][..],
            true,
            None,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
    ] {
        let code = [&[0x01, 0xd1][..], faulting].concat();
        let mut image = image(&code);
        image.cpu.registers.eax = 1;
        image.cpu.registers.ecx = 0x7fff_fffe;
        image.cpu.registers.edx = 2;
        image.cpu.registers.ebx = 0x4ffe;
        image.map(4, 0x8000, first_writable);
        if let Some(writable) = second_page {
            image.map(5, 0xa000, writable);
        }
        image.data(0x8ffd, &[0xa5, 0xff, 0xff]);
        image.data(0xa000, &[0xff, 0xff, 0x5a]);
        let mut completed = image.cpu;
        completed.flags.kind = 10;
        completed.flags.left = 0x7fff_fffe;
        completed.flags.right = 2;
        completed.registers.ecx = 0x8000_0000;
        completed.eip = 0x1002;
        completed.instruction_count = 0;
        both(
            step,
            name,
            &code,
            2,
            &image,
            &[
                Step {
                    cpu: completed,
                    ram: &[],
                    exit: Exit::Dispatch(0x1002),
                },
                Step {
                    cpu: completed,
                    ram: &[],
                    exit: fault,
                },
            ],
        );
    }
    let code = [0x0f, 0x94, 0x03];
    let mut image = image(&code);
    image.cpu.registers.ebx = 0x4000;
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0xa5]);
    both(
        step,
        "SETcc denied destination preserves incoming flags",
        &code,
        1,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00004000,
                error: 0x3,
            },
        }],
    );
}
