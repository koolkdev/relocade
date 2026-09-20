//! Selector verification observes the current host table without loading caches.

#[path = "verification/operands.rs"]
mod operands;
#[path = "verification/progress.rs"]
mod progress;

use super::{
    data,
    selector_cases::{code_defaults, descriptor},
};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentPermissionQuery, TestModule},
};
use crate::{
    CpuState, DescriptorTables, Gpr32, PrivilegeLevel, Segment, SegmentDefaultSize,
    SegmentDescriptor, SegmentDescriptorKind, SegmentProfile,
};

fn image(code: &[u8], profile: SegmentProfile) -> Image {
    let mut image = Image::new(code);
    code_defaults(&mut image, profile);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.cf = 1;
    image.cpu.flags.bytes.pf = 0;
    image.cpu.flags.bytes.af = 1;
    image.cpu.flags.bytes.zf = 0;
    image.cpu.flags.bytes.sf = 0;
    image.cpu.flags.bytes.of = 1;
    image
}

fn completed(image: &Image, len: usize, zf: bool) -> CpuState {
    let mut cpu = image.cpu;
    cpu.flags.bytes.zf = u8::from(zf);
    cpu.eip += len as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn check_one(
    engine: Engine,
    profile: SegmentProfile,
    code: &[u8],
    image: &Image,
    queries: &[SegmentPermissionQuery],
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, profile);
    let mut input = image.input();
    input.segment_permission_queries = queries.to_vec();
    let mut wanted = expected(image, &[step]);
    for (index, query) in queries.iter().enumerate() {
        wanted.events.insert(
            index,
            Event::SegmentPermissions {
                selector: i32::from(query.selector),
            },
        );
    }
    for module in [block, TestModule::interpreter_with_profile(profile)] {
        assert_eq!(
            engine.observe(module, &input, 1),
            wanted,
            "{} {profile:?} {code:02x?}",
            module.entry
        );
    }
}

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
                &[SegmentPermissionQuery::new(&tables, selector)],
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
