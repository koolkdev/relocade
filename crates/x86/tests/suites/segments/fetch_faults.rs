use super::code;
use crate::support::{
    machine::{Exit, Image},
    step::TestModule,
};
use relocade_x86::{ExecutionProfile, SegmentProfile};

#[test]
fn required_bytes_preserve_cs_page_and_length_fault_order() {
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Protected(
        SegmentProfile::Segmented32,
    ));
    for (bytes, eip, limit, fault) in [
        // MOV EAX,imm32: a missing second immediate byte precedes a later CS fault.
        (
            vec![0xb8, 0x78],
            0xffe,
            0x1001,
            Exit::PageFault {
                address: 0x9000,
                error: 0x10,
            },
        ),
        // At the same byte, CS is checked before its missing page.
        (
            vec![0xb8, 0x78],
            0xffe,
            0xfff,
            Exit::GeneralProtection { error: 0 },
        ),
        // Prefix and extended-opcode continuations retain the same CS offset.
        (
            vec![0x64],
            0xfff,
            0xfff,
            Exit::GeneralProtection { error: 0 },
        ),
        (
            vec![0x64, 0x66, 0x0f],
            0xffd,
            0x1000,
            Exit::PageFault {
                address: 0x9000,
                error: 0x10,
            },
        ),
        // Byte 15 is required and absent; byte 16 must not mask that fault.
        (
            [vec![0x66; 13], vec![0xb8]].concat(),
            0xff2,
            0x1001,
            Exit::PageFault {
                address: 0x9000,
                error: 0x10,
            },
        ),
        (
            vec![0x66; 15],
            0xff1,
            0xfff,
            Exit::GeneralProtection { error: 0 },
        ),
    ] {
        let mut image = Image::empty();
        image.cpu.eip = eip;
        image.cpu.segments.cs = code(0x8000, limit);
        image.map(8, 0x3000, false);
        image.data(0x3000 + eip, &bytes);
        image.check_unchanged_exit(module, &format!("{bytes:02x?}, CS limit {limit:x}"), fault);
    }
    // Even with no code page, invalid CS permissions win at the first byte.
    for (usable, executable) in [(0, 1), (1, 0)] {
        let mut image = Image::empty();
        image.cpu.segments.cs = code(0x8000, u32::MAX);
        image.cpu.segments.cs.usable = usable;
        image.cpu.segments.cs.executable = executable;
        image.check_unchanged_exit(
            module,
            &format!("CS usable={usable}, executable={executable}"),
            Exit::GeneralProtection { error: 0 },
        );
    }
    // A legal CS offset can wrap in linear space; PF reports the linear byte.
    let mut image = Image::empty();
    image.cpu.eip = 0x20;
    image.cpu.segments.cs = code(0xffff_ffdf, 0x40);
    image.map(0xfffff, 0x3000, false);
    image.data(0x3fff, &[0xb8]);
    image.check_unchanged_exit(
        module,
        "instruction fetch wraps its linear address",
        Exit::PageFault {
            address: 0,
            error: 0x10,
        },
    );
}

#[test]
fn required_instruction_spans_exceed_cs_limit() {
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Protected(
        SegmentProfile::Segmented32,
    ));
    for bytes in [
        vec![0x90],
        vec![0xb8, 0x78, 0x56, 0x34, 0x12],
        vec![0x66, 0xb8, 0x78, 0x56],
        vec![0x64, 0x66, 0x0f, 0xb7, 0x43, 0x10],
        [vec![0x64; 12], vec![0x66, 0x89, 0xc0]].concat(),
    ] {
        let mut image = Image::empty();
        image.cpu.segments.cs = code(0x8000, 0x1000 + bytes.len() as u32 - 2);
        image.map(9, 0x3000, false);
        image.data(0x3000, &bytes);
        image.check_unchanged_exit(
            module,
            &format!("required final instruction byte exceeds CS: {bytes:02x?}"),
            Exit::GeneralProtection { error: 0 },
        );
    }
}
