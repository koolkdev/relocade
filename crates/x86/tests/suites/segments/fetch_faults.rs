use super::code;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{SegmentAttributes, SegmentProfile};

fn fault_order(engine: Engine) {
    let module = TestModule::interpreter_with_profile(SegmentProfile::Segmented32);
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
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: fault,
                }]
            ),
            "{bytes:02x?}, CS limit {limit:x}"
        );
    }
    // Even with no code page, invalid CS permissions win at the first byte.
    for bits in [0x10, 0x15, 0x1b] {
        let mut image = Image::empty();
        image.cpu.segments.cs = code(0x8000, u32::MAX);
        image.cpu.segments.cs.attributes = SegmentAttributes::from_bits(bits);
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }]
            )
        );
    }
    // A legal CS offset can wrap in linear space; PF reports the linear byte.
    let mut image = Image::empty();
    image.cpu.eip = 0x20;
    image.cpu.segments.cs = code(0xffff_ffdf, 0x40);
    image.map(0xfffff, 0x3000, false);
    image.data(0x3fff, &[0xb8]);
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0,
                    error: 0x10
                },
            }]
        )
    );
}

#[test]
fn required_bytes_preserve_cs_page_and_length_fault_order() {
    fault_order(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_required_bytes_preserve_cs_page_and_length_fault_order() {
    fault_order(Engine::V8);
}
