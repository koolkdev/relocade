use super::*;
use crate::{SegmentAttributes, SegmentDefaultSize, SegmentKind, StoredSegment};

fn word_destinations(engine: Engine) {
    for profile in PROFILES {
        for operand_override in [false, true] {
            let mut code = if operand_override { vec![0x66] } else { vec![] };
            if profile == SegmentProfile::Segmented16 {
                code.push(0x67);
            }
            code.extend([0x63, 0x0b]); // ARPL [EBX],CX.
            let mut image = image(&code, profile);
            image.cpu.registers.ebx = 0x4ffe;
            image.cpu.registers.ecx = 0xabcd_0003;
            if profile != SegmentProfile::Flat32 {
                image.cpu.segments.ds.limit = 0x4fff;
            }
            // Exactly two bytes fit before the segment/page end.
            image.map(4, 0x8000, true);
            image.data(0x8ffd, &[0x5a, 0xfc, 0xff]);
            let cpu = completed(&image, code.len(), true);
            check_one(
                engine,
                profile,
                &code,
                &image,
                Step {
                    cpu,
                    ram: &[(0x8ffe, &[0xff, 0xff])],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn memory_destinations_remain_words_with_either_code_size_and_operand_override() {
    word_destinations(Engine::Wasmtime);
}

fn overrides_and_aliases(engine: Engine) {
    for profile in [SegmentProfile::Segmented32, SegmentProfile::Segmented16] {
        let mut code = vec![0x64];
        if profile == SegmentProfile::Segmented32 {
            code.push(0x67);
        }
        code.extend([0x63, 0x42, 0]); // ARPL FS:[BP+SI],AX.
        let mut image = image(&code, profile);
        image.cpu.registers.eax = 3;
        image.cpu.registers.ebp = 0xabcd_fffe;
        image.cpu.registers.esi = 0xdead_0001;
        image.cpu.segments.fs = data(0x8000, 0x10000);
        image.cpu.segments.ss = StoredSegment::unusable(0);
        image.map(0x17, 0x8000, true);
        image.map(0x18, 0xa000, true);
        image.data(0x8ffe, &[0x5a, 0xfc]);
        image.data(0xa000, &[0xff, 0x5a]);
        let cpu = completed(&image, code.len(), true);
        check_one(
            engine,
            profile,
            &code,
            &image,
            Step {
                cpu,
                ram: &[(0x8fff, &[0xff]), (0xa000, &[0xff])],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
    let profile = SegmentProfile::Flat32;
    let code = [0x63, 0x1b]; // ARPL [EBX],BX.
    let mut image = image(&code, profile);
    image.cpu.registers.ebx = 0x4003;
    image.map(4, 0x8000, true);
    image.data(0x8002, &[0x5a, 0x20, 0xf3, 0x5a]);
    let cpu = completed(&image, code.len(), true);
    check_one(
        engine,
        profile,
        &code,
        &image,
        Step {
            cpu,
            ram: &[(0x8003, &[0x23, 0xf3])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

#[test]
fn address_and_segment_overrides_split_pages_and_source_address_aliases_work() {
    overrides_and_aliases(Engine::Wasmtime);
}

fn page_faults(engine: Engine) {
    let code = [0x63, 0x0b]; // ARPL [EBX],CX.
    for (old_selector, source_rpl) in [(0x20, 3), (0x23, 3), (0x23, 1)] {
        for (address, first, second, fault_address, error) in [
            (0x4020, None, None, 0x4020, 2),
            (0x4020, Some(false), None, 0x4020, 3),
            (0x4fff, Some(true), None, 0x5000, 2),
            (0x4fff, Some(true), Some(false), 0x5000, 3),
        ] {
            let profile = SegmentProfile::Flat32;
            let mut image = image(&code, profile);
            image.cpu.registers.ebx = address;
            image.cpu.registers.ecx = source_rpl;
            image.cpu.flags.status_source.kind = 10;
            image.cpu.flags.status_source.left = 0x7fff_ffff;
            image.cpu.flags.status_source.right = 1;
            if let Some(writable) = first {
                image.map(4, 0x8000, writable);
            }
            if let Some(writable) = second {
                image.map(5, 0xa000, writable);
            }
            image.data(0x8020, &[old_selector, 0]);
            image.data(0x8fff, &[old_selector]);
            image.data(0xa000, &[0]);
            check_one(
                engine,
                profile,
                &code,
                &image,
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: fault_address,
                        error,
                    },
                },
            );
        }
    }
}

#[test]
fn both_comparison_outcomes_require_the_complete_write_span_before_changing_flags() {
    page_faults(Engine::Wasmtime);
}

fn segment_faults(engine: Engine) {
    let read_only = StoredSegment {
        attributes: SegmentAttributes::new(
            SegmentKind::Data {
                writable: false,
                expand_down: false,
            },
            SegmentDefaultSize::Bits32,
        ),
        ..data(0x4000, 0xffff)
    };
    for (old_selector, source_rpl) in [(0x20, 3), (0x23, 3), (0x23, 1)] {
        for stack in [false, true] {
            for segment in [data(0x4000, 0xfff), read_only, StoredSegment::unusable(0)] {
                let code = if stack {
                    vec![0x63, 0x0c, 0x24] // ARPL [ESP],CX.
                } else {
                    vec![0x63, 0x0b] // ARPL [EBX],CX.
                };
                let profile = SegmentProfile::Segmented32;
                let mut image = image(&code, profile);
                image.cpu.registers.ebx = 0xfff;
                image.cpu.registers.esp = 0xfff;
                image.cpu.registers.ecx = source_rpl;
                image.cpu.flags.status_source.kind = 10;
                image.cpu.flags.status_source.left = 0x7fff_ffff;
                image.cpu.flags.status_source.right = 1;
                if stack {
                    image.cpu.segments.ss = segment;
                } else {
                    image.cpu.segments.ds = segment;
                }
                // The segment fault wins over the missing second page.
                image.map(4, 0x8000, true);
                image.data(0x8fff, &[old_selector]);
                check_one(
                    engine,
                    profile,
                    &code,
                    &image,
                    Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: if stack {
                            Exit::StackFault { error: 0 }
                        } else {
                            Exit::GeneralProtection { error: 0 }
                        },
                    },
                );
            }
        }
    }
}

#[test]
fn segment_write_guards_precede_paging_and_preserve_the_restart_state() {
    segment_faults(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_arpl_memory_widths_overrides_aliases_and_faults() {
    word_destinations(Engine::V8);
    overrides_and_aliases(Engine::V8);
    page_faults(Engine::V8);
    segment_faults(Engine::V8);
}
