use super::*;

fn call_fault_order(engine: Engine) {
    for (present, stack_limit, target_limit, exit) in [
        (false, 0x8fff, 0x1ff, Exit::Other(0x0020_0024_0000_0000)),
        (true, 0x8fff, 0x1ff, Exit::StackFault { error: 0 }),
        (true, 0x9007, 0x1ff, Exit::GeneralProtection { error: 0 }),
        (
            true,
            0x9007,
            0x200,
            Exit::PageFault {
                address: 0x9004,
                error: 2,
            },
        ),
    ] {
        for indirect in [false, true] {
            let code = if indirect {
                vec![0xff, 0x1b]
            } else {
                immediate(false, 0x200, 0x27)
            };
            let mut image = Image::new(&code);
            image.cpu.registers.esp = 0x9008;
            image.cpu.registers.ebx = 0x4000;
            image.cpu.segments.ss.limit = stack_limit;
            image.map(4, 0x5000, false);
            image.data(0x5000, &pointer(false, 0x200, 0x27));
            let mut table = tables(target_limit);
            table.insert(
                0x27,
                SegmentDescriptor {
                    present,
                    ..descriptor(0xc000, target_limit, SegmentDefaultSize::Bits32)
                },
            );
            check_one(
                engine,
                SegmentProfile::Segmented32,
                &code,
                &image,
                &[SegmentResolution::new(&table, Segment::Cs, 0x27)],
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
fn call_checks_descriptor_then_stack_capacity_then_target_then_stack_paging() {
    call_fault_order(Engine::Wasmtime);
}

fn frame_page_faults(engine: Engine) {
    for returning in [false, true] {
        for word in [false, true] {
            let code = if returning {
                ret(word, Some(0xffff))
            } else {
                immediate(word, 0x200, 0x27)
            };
            // Offset on page 8; selector on page 9. Neither half may be written
            // when either page is absent or refuses the write.
            for (first, second, error, address) in [
                (
                    None,
                    Some(true),
                    if returning { 0 } else { 2 },
                    if word { 0x8ffe } else { 0x8ffc },
                ),
                (Some(true), None, if returning { 0 } else { 2 }, 0x9000),
                (Some(true), Some(false), 3, 0x9000),
                (
                    Some(false),
                    Some(true),
                    3,
                    if word { 0x8ffe } else { 0x8ffc },
                ),
                (
                    None,
                    None,
                    if returning { 0 } else { 2 },
                    if returning {
                        if word {
                            0x8ffe
                        } else {
                            0x8ffc
                        }
                    } else {
                        0x9000
                    },
                ),
            ] {
                if returning && error == 3 {
                    continue;
                }
                let start = if word { 0x8ffe } else { 0x8ffc };
                let mut image = Image::new(&code);
                image.cpu.registers.esp = if returning {
                    start
                } else if word {
                    0x9002
                } else {
                    0x9004
                };
                if let Some(write) = first {
                    image.map(8, 0x8000, write);
                }
                if let Some(write) = second {
                    image.map(9, 0x5000, write);
                }
                image.data(0x8ffc, &[0xa5; 4]);
                image.data(0x5000, &[0xa5; 4]);
                if returning {
                    image.data(
                        if word { 0x8ffe } else { 0x8ffc },
                        if word { &[0, 2][..] } else { &[0, 2, 0, 0] },
                    );
                    image.data(0x5000, &[0x27, 0]);
                }
                let resolutions = if returning {
                    vec![]
                } else {
                    vec![SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)]
                };
                check_one(
                    engine,
                    SegmentProfile::Flat32,
                    &code,
                    &image,
                    &resolutions,
                    Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::PageFault { address, error },
                    },
                );
            }
        }
    }
}

#[test]
fn every_frame_page_is_proved_before_stores_or_selector_resolution_for_return() {
    frame_page_faults(Engine::Wasmtime);
}

fn return_selectors(engine: Engine) {
    let normal = descriptor(0xc000, 0x200, SegmentDefaultSize::Bits32);
    for (selector, slot, resolved, exit) in [
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
                present: false,
                kind: SegmentDescriptorKind::Data {
                    writable: true,
                    expand_down: false,
                },
                ..normal
            }),
            true,
            Exit::GeneralProtection { error: 0x24 },
        ),
        (
            0x27,
            Some(SegmentDescriptor {
                present: false,
                dpl: PrivilegeLevel::Ring0,
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
                limit: 0x1ff,
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
                dpl: PrivilegeLevel::Ring0,
                ..normal
            }),
            true,
            Exit::Dispatch(0x200),
        ),
    ] {
        for word in [false, true] {
            let code = ret(word, Some(0xffff));
            let mut image = image_with_stack(&code, 0x9000);
            image.data(0x8000, &pointer(word, 0x200, selector));
            let mut table = DescriptorTables::default();
            if let Some(slot) = slot {
                table.insert(selector, slot);
            }
            let mut cpu = image.cpu;
            if let Exit::Dispatch(target) = exit {
                cpu.eip = target;
                cpu.segments.cs =
                    loaded(selector, 0xc000, 0x200, if selector == 7 { 19 } else { 23 });
                cpu.registers.esp = if word { 0x19003 } else { 0x19007 };
                cpu.instruction_count = 0;
            }
            let resolutions = if resolved {
                vec![SegmentResolution::new(&table, Segment::Cs, selector)]
            } else {
                vec![]
            };
            check_one(
                engine,
                SegmentProfile::Flat32,
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
fn return_requires_rpl_three_before_reusing_code_descriptor_rules_and_limit_checks() {
    return_selectors(Engine::Wasmtime);
}

fn earlier_progress(engine: Engine) {
    for returning in [false, true] {
        // The preceding store supplies the target offset. Target 2 exceeds limit 1.
        let code = [
            &[0x83, 0xc0, 1, 0x89, 0x03][..],
            if returning {
                &[0xcb][..]
            } else {
                &[0xff, 0x1b][..]
            },
        ]
        .concat();
        let mut image = image_with_stack(&code, if returning { 0x9000 } else { 0x9010 });
        image.cpu.registers.eax = 1;
        image.cpu.registers.ebx = 0x9000;
        image.data(0x8004, &[0x27, 0]);
        let mut input = image.input();
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables(1), Segment::Cs, 0x27));
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
            exit: Exit::GeneralProtection { error: 0 },
        };
        let event = Event::ResolveSegment {
            segment: 1,
            selector: 0x27,
        };
        let mut wanted = expected(&image, &[first, second, fault()]);
        wanted.events.insert(4, event.clone());
        assert_eq!(engine.observe(TestModule::interpreter(), &input, 3), wanted);
        let mut blocks = BlockModules::default();
        let module = blocks.get(&image.cpu, &code, 3, SegmentProfile::Flat32);
        let mut wanted = expected(&image, &[fault()]);
        wanted.events.insert(0, event);
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }
}

#[test]
fn faults_publish_earlier_register_flag_memory_and_retirement_progress() {
    earlier_progress(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_call_return_faults_and_restart() {
    call_fault_order(Engine::V8);
    frame_page_faults(Engine::V8);
    return_selectors(Engine::V8);
    earlier_progress(Engine::V8);
}
