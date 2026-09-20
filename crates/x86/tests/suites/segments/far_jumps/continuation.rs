use super::*;

fn new_code_defaults(engine: Engine) {
    for next_word in [false, true] {
        let code = immediate(false, 0x200, 0x1b); // Reload the currently visible selector.
        let mut image = Image::new(&code);
        image.map(9, 0x8000, false);
        let next_code = if next_word {
            &[0xb8, 0x78, 0x56][..]
        } else {
            &[0xb8, 0x78, 0x56, 0x34, 0x92]
        };
        image.data(0x8200, next_code);
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x1b,
            SegmentDescriptor {
                kind: SegmentDescriptorKind::Code {
                    readable: false,
                    conforming: false,
                },
                ..descriptor(
                    0x9000,
                    0xffff,
                    if next_word {
                        SegmentDefaultSize::Bits16
                    } else {
                        SegmentDefaultSize::Bits32
                    },
                )
            },
        );
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x1b, 0x9000, 0xffff, if next_word { 3 } else { 19 });
        cpu.eip = 0x200;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &[SegmentResolution::new(&tables, Segment::Cs, 0x1b)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
        // Dispatch admits a fresh entry whose defaults and fetch base match the new CS.
        image.cpu = cpu;
        cpu.registers.eax = if next_word { 0x1111_5678 } else { 0x9234_5678 };
        cpu.eip += next_code.len() as u32;
        cpu.instruction_count = 1;
        check_one(
            engine,
            if next_word {
                SegmentProfile::Segmented16
            } else {
                SegmentProfile::Segmented32
            },
            next_code,
            &image,
            &[],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn reloading_cs_changes_the_next_fetch_base_and_decode_defaults() {
    new_code_defaults(Engine::Wasmtime);
}

fn destination_paging(engine: Engine) {
    let code = immediate(false, 0x200, 0x27);
    let mut image = Image::new(&code);
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0x9000, 0xffff, SegmentDefaultSize::Bits32));
    let mut cpu = image.cpu;
    cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, 23);
    cpu.eip = 0x200;
    cpu.instruction_count = 0;
    check_one(
        engine,
        SegmentProfile::Flat32,
        &code,
        &image,
        &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
    image.cpu = cpu;
    assert_eq!(
        engine.observe(
            TestModule::interpreter_with_profile(SegmentProfile::Segmented32),
            &image.input(),
            1
        ),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x9200,
                    error: 0x10
                }
            }]
        )
    );
}

#[test]
fn destination_paging_happens_on_the_next_fetch_after_the_jump_retires() {
    destination_paging(Engine::Wasmtime);
}

fn terminal_jump(engine: Engine) {
    for indirect in [false, true] {
        let mut code = if indirect {
            vec![0xff, 0x2b]
        } else {
            immediate(false, 0x200, 0x27)
        };
        let length = code.len();
        code.push(0xf4); // Unsupported old-code byte must not be decoded.
        let mut image = Image::new(&code);
        image.cpu.segments.cs.limit = 0x1000 + length as u32 - 1;
        image.cpu.registers.ebx = 0x4000;
        image.map(4, 0x8000, false);
        image.data(0x8000, &pointer(false, 0x200, 0x27));
        let mut tables = DescriptorTables::default();
        tables.insert(0x27, descriptor(0x9000, 0xffff, SegmentDefaultSize::Bits32));
        let mut input = image.input();
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables, Segment::Cs, 0x27));
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, 23);
        cpu.eip = 0x200;
        cpu.instruction_count = 0;
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        wanted.events.insert(
            0,
            Event::ResolveSegment {
                segment: 1,
                selector: 0x27,
            },
        );
        let mut blocks = BlockModules::default();
        for module in [
            blocks.get(&image.cpu, &code, 2, SegmentProfile::Segmented32),
            TestModule::interpreter_with_profile(SegmentProfile::Segmented32),
        ] {
            assert_eq!(engine.observe(module, &input, 1), wanted);
        }
    }
}

#[test]
fn both_far_jump_forms_end_the_block_without_fetching_old_fallthrough() {
    terminal_jump(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_jump_dispatch_and_continuation() {
    new_code_defaults(Engine::V8);
    destination_paging(Engine::V8);
    terminal_jump(Engine::V8);
}
