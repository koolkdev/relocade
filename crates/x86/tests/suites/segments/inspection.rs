//! LAR/LSL inspect descriptors without loading a segment or ending a block.

#[path = "inspection/operands.rs"]
mod operands;
#[path = "inspection/progress.rs"]
mod progress;

use super::{
    data,
    inspection_cases::{check_one, completed, image},
    selector_cases::descriptor,
};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Step},
    step::{Engine, Event, SegmentQuery, TestModule},
};
use crate::{
    DescriptorTables, Gpr32, PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, SegmentLimit, SegmentProfile,
};

fn descriptor_results(engine: Engine) {
    use SegmentDescriptorKind::{Code, Data};
    let mut tables = DescriptorTables::default();
    let ordinary = descriptor(0x1234_0000, SegmentDefaultSize::Bits16);
    tables.insert(0, ordinary);
    tables.insert(
        4,
        SegmentDescriptor {
            present: false,
            available: true,
            default_size: SegmentDefaultSize::Bits32,
            kind: Data {
                writable: true,
                expand_down: true,
            },
            limit: SegmentLimit::pages(0x12345).unwrap(),
            ..ordinary
        },
    );
    tables.insert(
        0x27,
        SegmentDescriptor {
            dpl: PrivilegeLevel::Ring2,
            ..ordinary
        },
    );
    tables.insert(
        0x2b,
        SegmentDescriptor {
            kind: Code {
                readable: false,
                conforming: true,
            },
            dpl: PrivilegeLevel::Ring0,
            present: false,
            ..ordinary
        },
    );
    tables.insert(
        0x2f,
        SegmentDescriptor {
            kind: Code {
                readable: false,
                conforming: false,
            },
            ..ordinary
        },
    );
    tables.insert(
        0xffff,
        SegmentDescriptor {
            limit: SegmentLimit::pages(0xfffff).unwrap(),
            ..ordinary
        },
    );
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        for (selector, result) in [
            (0, None),
            (3, None),
            (7, Some((0x00d0_7700, 0x1234_5fff))),
            (0x27, None),
            (0x2b, Some((0x1d00, 0xffff))),
            (0x2f, Some((0xf900, 0xffff))),
            (0x33, None),
            (0xffff, Some((0x0080_f300, u32::MAX))),
        ] {
            for opcode in [0x02, 0x03] {
                let code = [0x0f, opcode, 0xc8]; // LAR/LSL (E)CX,AX.
                let mut image = image(&code, profile);
                image.cpu.registers.eax = 0xbeef_0000 | u32::from(selector);
                image.cpu.registers.ecx = 0xcafe_1234;
                image.cpu.flags.bytes.zf = u8::from(result.is_none());
                let mut cpu = completed(&image, code.len(), result.is_some());
                if let Some((rights, limit)) = result {
                    let value = if opcode == 2 { rights } else { limit };
                    cpu.registers.ecx = if profile == SegmentProfile::Segmented16 {
                        0xcafe_0000 | (value & 0xffff)
                    } else {
                        value
                    };
                }
                check_one(
                    engine,
                    profile,
                    &code,
                    &image,
                    &[SegmentQuery::new(&tables, selector)],
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip),
                    },
                );
            }
        }
    }
}

#[test]
fn lar_and_lsl_report_fields_or_preserve_the_destination_for_rejected_selectors() {
    descriptor_results(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_lar_and_lsl_report_fields_or_preserve_the_destination_for_rejected_selectors() {
    descriptor_results(Engine::V8);
}
