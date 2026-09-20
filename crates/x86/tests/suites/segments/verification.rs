//! Selector verification observes the current host table without loading caches.

#[path = "verification/operands.rs"]
mod operands;
#[path = "verification/progress.rs"]
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
    CpuState, DescriptorTables, Gpr32, PrivilegeLevel, Segment, SegmentDefaultSize,
    SegmentDescriptor, SegmentDescriptorKind, SegmentProfile,
};

fn descriptor_results(engine: Engine) {
    use SegmentDescriptorKind::{Code, Data};
    let profile = SegmentProfile::Flat32;
    let mut tables = DescriptorTables::default();
    for (selector, kind, dpl, present) in [
        (
            0,
            Data {
                writable: true,
                expand_down: false,
            },
            PrivilegeLevel::Ring3,
            true,
        ),
        (
            4,
            Data {
                writable: true,
                expand_down: true,
            },
            PrivilegeLevel::Ring3,
            false,
        ),
        (
            0x20,
            Data {
                writable: false,
                expand_down: false,
            },
            PrivilegeLevel::Ring3,
            true,
        ),
        (
            0x24,
            Data {
                writable: true,
                expand_down: false,
            },
            PrivilegeLevel::Ring2,
            true,
        ),
        (
            0x28,
            Code {
                readable: true,
                conforming: true,
            },
            PrivilegeLevel::Ring0,
            false,
        ),
        (
            0x2c,
            Code {
                readable: false,
                conforming: true,
            },
            PrivilegeLevel::Ring3,
            true,
        ),
        (
            0x30,
            Code {
                readable: true,
                conforming: false,
            },
            PrivilegeLevel::Ring2,
            true,
        ),
        (
            0x34,
            Code {
                readable: true,
                conforming: false,
            },
            PrivilegeLevel::Ring3,
            true,
        ),
    ] {
        tables.insert(
            selector,
            SegmentDescriptor {
                kind,
                dpl,
                present,
                ..descriptor(0, SegmentDefaultSize::Bits32)
            },
        );
    }
    for (selector, readable, writable) in [
        (0, false, false),
        (3, false, false),
        (4, true, true),
        (7, true, true),
        (0x23, true, false),
        (0x27, false, false),
        (0x2b, true, false),
        (0x2f, false, false),
        (0x33, false, false),
        (0x37, true, false),
        (0xffff, false, false),
    ] {
        for (extension, result) in [(4, readable), (5, writable)] {
            let code = [0x0f, 0x00, 0xc0 | (extension << 3)];
            let mut image = image(&code, profile);
            image.cpu.registers.eax = 0xabcd_0000 | u32::from(selector);
            image.cpu.flags.bytes.zf = u8::from(!result);
            let cpu = completed(&image, code.len(), result);
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

#[test]
fn verification_reports_rights_in_zf_without_loading_or_faulting_for_a_selector() {
    descriptor_results(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_verification_reports_rights_in_zf_without_loading_or_faulting_for_a_selector() {
    descriptor_results(Engine::V8);
}
