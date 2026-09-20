use super::*;

fn source_faults(engine: Engine) {
    for stack_source in [false, true] {
        for word in [false, true] {
            for (limit, first_present, exit) in [
                (
                    0xfff,
                    true,
                    if stack_source {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::GeneralProtection { error: 0 }
                    },
                ),
                (
                    0xfff,
                    false,
                    if stack_source {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::GeneralProtection { error: 0 }
                    },
                ),
                (
                    0xffff,
                    true,
                    Exit::PageFault {
                        address: 0x5000,
                        error: 0,
                    },
                ),
                (
                    0xffff,
                    false,
                    Exit::PageFault {
                        address: if word { 0x4ffe } else { 0x4ffc },
                        error: 0,
                    },
                ),
            ] {
                let mut code = if word { vec![0x66, 0xff] } else { vec![0xff] };
                code.extend(if stack_source {
                    &[0x6d, 0][..]
                } else {
                    &[0x2b]
                });
                let mut image = Image::new(&code);
                image.cpu.registers.ebx = if word { 0xffe } else { 0xffc };
                image.cpu.registers.ebp = image.cpu.registers.ebx;
                image.cpu.segments[if stack_source {
                    Segment::Ss
                } else {
                    Segment::Ds
                }] = data(0x4000, limit);
                if first_present {
                    image.map(4, 0x8000, false);
                }
                // The offset fits on page one, but the selector is on page two.
                image.data(
                    if word { 0x8ffe } else { 0x8ffc },
                    if word { &[0, 2][..] } else { &[0, 2, 0, 0] },
                );
                check_one(
                    engine,
                    SegmentProfile::Segmented32,
                    &code,
                    &image,
                    &[],
                    Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit,
                    },
                );
            }
        }
    }
}

#[test]
fn the_complete_source_span_is_checked_before_paging_and_selector_resolution() {
    source_faults(Engine::Wasmtime);
}

fn descriptor_faults(engine: Engine) {
    let code_descriptor = descriptor(0x9000, 0x10, SegmentDefaultSize::Bits32);
    for (selector, slot, exit) in [
        (3, None, Exit::GeneralProtection { error: 0 }),
        (0xf327, None, Exit::GeneralProtection { error: 0xf324 }),
        (
            0xf327,
            Some(SegmentDescriptor {
                kind: SegmentDescriptorKind::Data {
                    writable: true,
                    expand_down: false,
                },
                present: false,
                ..code_descriptor
            }),
            Exit::GeneralProtection { error: 0xf324 },
        ),
        (
            0xf327,
            Some(SegmentDescriptor {
                dpl: PrivilegeLevel::Ring0,
                present: false,
                ..code_descriptor
            }),
            Exit::GeneralProtection { error: 0xf324 },
        ),
        (
            0xf327,
            Some(SegmentDescriptor {
                present: false,
                ..code_descriptor
            }),
            Exit::Other(0x0020_f324_0000_0000),
        ),
    ] {
        for indirect in [false, true] {
            // The target also exceeds the new limit; descriptor faults must win.
            let code = if indirect {
                vec![0xff, 0x2b]
            } else {
                immediate(false, 0x200, selector)
            };
            let mut image = Image::new(&code);
            image.cpu.registers.ebx = 0x4000;
            image.map(4, 0x8000, false);
            image.data(0x8000, &pointer(false, 0x200, selector));
            let mut tables = DescriptorTables::default();
            if let Some(slot) = slot {
                tables.insert(selector, slot);
            }
            check_one(
                engine,
                SegmentProfile::Flat32,
                &code,
                &image,
                &[SegmentResolution::new(&tables, Segment::Cs, selector)],
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit,
                },
            );
        }
    }
}

#[test]
fn descriptor_type_privilege_and_presence_faults_precede_the_new_target_limit() {
    descriptor_faults(Engine::Wasmtime);
}

fn earlier_progress(engine: Engine) {
    for present in [false, true] {
        // ADD EAX,1; MOV [EBX],EAX; JMP FAR [EBX]. Target 2 exceeds limit 1.
        let code = [0x83, 0xc0, 1, 0x89, 0x03, 0xff, 0x2b];
        let mut image = Image::new(&code);
        image.cpu.registers.eax = 1;
        image.cpu.registers.ebx = 0x4000;
        image.map(4, 0x8000, true);
        image.data(0x8004, &[0x27, 0]);
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x27,
            SegmentDescriptor {
                present,
                ..descriptor(0x9000, 1, SegmentDefaultSize::Bits32)
            },
        );
        let mut input = image.input();
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables, Segment::Cs, 0x27));
        let mut cpu = image.cpu;
        cpu.registers.eax = 2;
        cpu.flags.status_source.kind = 10;
        cpu.flags.status_source.left = 1;
        cpu.flags.status_source.right = 1;
        cpu.eip = 0x1003;
        cpu.instruction_count = 0;
        let first = Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        };
        cpu.eip = 0x1005;
        cpu.instruction_count = 1;
        let ram: &[(u32, &[u8])] = &[(0x8000, &[2, 0, 0, 0])];
        let second = Step {
            cpu,
            ram,
            exit: Exit::Dispatch(cpu.eip),
        };
        let fault = || Step {
            cpu,
            ram,
            exit: if present {
                Exit::GeneralProtection { error: 0 }
            } else {
                Exit::Other(0x0020_0024_0000_0000)
            },
        };
        let event = Event::ResolveSegment {
            segment: 1,
            selector: 0x27,
        };
        let mut wanted = expected(&image, &[first, second, fault()]);
        wanted.events.insert(4, event.clone());
        assert_eq!(engine.observe(TestModule::interpreter(), &input, 3), wanted);
        let mut blocks = BlockModules::default();
        let block = blocks.get(&image.cpu, &code, 3, SegmentProfile::Flat32);
        let mut wanted = expected(&image, &[fault()]);
        wanted.events.insert(0, event);
        assert_eq!(engine.observe(block, &input, 1), wanted);
    }
}

#[test]
fn failed_jumps_publish_earlier_register_flag_memory_and_retirement_progress() {
    earlier_progress(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_jump_faults_and_restart() {
    source_faults(Engine::V8);
    descriptor_faults(Engine::V8);
    earlier_progress(Engine::V8);
}
