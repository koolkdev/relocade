use super::*;

fn stack_faults(engine: Engine) {
    for word in [false, true] {
        let code = if word { vec![0x66, 0xcf] } else { vec![0xcf] };
        for (start, limit, mapped, fault_address, capacity_fault) in [
            (0x6000u32, u32::MAX, false, 0x6000, false),
            (0x4fff, u32::MAX, true, 0x5000, false),
            (
                if word { 0x4ffe } else { 0x4ffc },
                u32::MAX,
                true,
                0x5000,
                false,
            ),
            (
                if word { 0x4ffc } else { 0x4ffa },
                u32::MAX,
                true,
                if word { 0x5000 } else { 0x5002 },
                false,
            ),
            (if word { 0x4ffc } else { 0x4ffa }, 0x4fff, true, 0, true),
        ] {
            let profile = SegmentProfile::Segmented32;
            let mut image = image(&code, profile);
            image.cpu.registers.esp = start;
            image.cpu.segments.ss.limit = limit;
            if mapped {
                image.map(4, 0xa000, false);
                // Invalid selector and target must not precede the last field read.
                let bytes = frame(word, u32::MAX, 0x24, u32::MAX);
                let available = (0x5000 - start) as usize;
                image.data(0xa000 + (start & 0xfff), &bytes[..available]);
            }
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[],
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: if capacity_fault {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::PageFault {
                            address: fault_address,
                            error: 0,
                        }
                    },
                },
            );
        }
    }
}

#[test]
fn complete_frame_capacity_and_all_three_reads_precede_selector_validation() {
    stack_faults(Engine::Wasmtime);
}

fn selector_faults(engine: Engine) {
    let normal = descriptor(0xc000, 0x200, SegmentDefaultSize::Bits32);
    for (selector, descriptor, resolved, exit) in [
        (
            0x24,
            Some(normal),
            false,
            Exit::GeneralProtection { error: 0x24 },
        ),
        (
            0x25,
            Some(normal),
            false,
            Exit::GeneralProtection { error: 0x24 },
        ),
        (
            0x26,
            Some(normal),
            false,
            Exit::GeneralProtection { error: 0x24 },
        ),
        (3, None, true, Exit::GeneralProtection { error: 0 }),
        (
            0xf327,
            None,
            true,
            Exit::GeneralProtection { error: 0xf324 },
        ),
        (
            0x27,
            Some(SegmentDescriptor {
                kind: SegmentDescriptorKind::Data {
                    writable: true,
                    expand_down: false,
                },
                present: false,
                ..normal
            }),
            true,
            Exit::GeneralProtection { error: 0x24 },
        ),
        (
            0x27,
            Some(SegmentDescriptor {
                dpl: crate::PrivilegeLevel::Ring0,
                present: false,
                ..normal
            }),
            true,
            Exit::GeneralProtection { error: 0x24 },
        ),
        (
            0x27,
            Some(SegmentDescriptor {
                present: false,
                ..normal
            }),
            true,
            Exit::Other(0x0020_0024_0000_0000),
        ),
        (
            0x27,
            Some(SegmentDescriptor {
                limit: crate::SegmentLimit::bytes(0x1ff).unwrap(),
                ..normal
            }),
            true,
            Exit::GeneralProtection { error: 0 },
        ),
        (0x27, Some(normal), true, Exit::Dispatch(0x200)),
        (
            7,
            Some(SegmentDescriptor {
                kind: SegmentDescriptorKind::Code {
                    readable: false,
                    conforming: true,
                },
                dpl: crate::PrivilegeLevel::Ring0,
                ..normal
            }),
            true,
            Exit::Dispatch(0x200),
        ),
    ] {
        for word in [false, true] {
            let code = if word { vec![0x66, 0xcf] } else { vec![0xcf] };
            let profile = SegmentProfile::Flat32;
            let mut image = image(&code, profile);
            image.data(0x8000, &frame(word, 0x200, selector, 0));
            let mut tables = DescriptorTables::default();
            if let Some(descriptor) = descriptor {
                tables.insert(selector, descriptor);
            }
            let resolutions = if resolved {
                vec![SegmentResolution::new(&tables, Segment::Cs, selector)]
            } else {
                vec![]
            };
            let mut cpu = image.cpu;
            if let Exit::Dispatch(eip) = exit {
                cpu = cleared_flags(cpu, word);
                cpu.eip = eip;
                cpu.segments.cs =
                    loaded(selector, 0xc000, 0x200, if selector == 7 { 19 } else { 23 });
                cpu.registers.esp = if word { 0x9006 } else { 0x900c };
                cpu.instruction_count = 0;
            }
            check_one(
                engine,
                profile,
                &code,
                &image,
                &resolutions,
                Step {
                    cpu,
                    ram: &[],
                    exit,
                },
            );
        }
    }
}

#[test]
fn return_selector_and_target_faults_preserve_flags_stack_pointer_and_loaded_cs() {
    selector_faults(Engine::Wasmtime);
}

fn nested_task(engine: Engine) {
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        for nt in [1, 0xff] {
            let code = [0x66, 0x67, 0xcf];
            let mut image = image(&code, profile);
            image.cpu.flags.bytes.nt = nt;
            image.cpu.registers.esp = 0x4000;
            if profile != SegmentProfile::Flat32 {
                image.cpu.segments.ss = crate::StoredSegment::unusable(0);
            }
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[],
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0008_00cf_0000_1000),
                },
            );
        }
    }
}

#[test]
fn entry_nt_reports_unsupported_before_stack_checks_without_retiring() {
    nested_task(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interrupt_return_fault_order_and_nested_task_exit() {
    stack_faults(Engine::V8);
    selector_faults(Engine::V8);
    nested_task(Engine::V8);
}
