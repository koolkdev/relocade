use super::*;

fn round_trip(engine: Engine) {
    for word in [false, true] {
        let call = immediate(word, 0x200, 0x27);
        let return_code = ret(!word, None); // Callee CS.D=0, match the CALL operand size.
        let mut image = image_with_stack(&call, 0x9008);
        image.map(0xc, 0x5000, false);
        image.data(0x5200, &return_code);
        let mut table = tables(0xffff);
        table.insert(0x27, descriptor(0xc000, 0xffff, SegmentDefaultSize::Bits16));
        table.insert(0x1b, descriptor(0, u32::MAX, SegmentDefaultSize::Bits32));
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 7);
        cpu.eip = 0x200;
        cpu.registers.esp = if word { 0x9004 } else { 0x9000 };
        cpu.instruction_count = 0;
        let saved = pointer(word, 0x1000 + call.len() as u32, 0x1b);
        let saved_address = if word { 0x8004 } else { 0x8000 };
        check_one(
            engine,
            SegmentProfile::Flat32,
            &call,
            &image,
            &[SegmentResolution::new(&table, Segment::Cs, 0x27)],
            Step {
                cpu,
                ram: &[(saved_address, &saved)],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
        image.cpu = cpu;
        image.data(saved_address, &saved);
        cpu.segments.cs = loaded(0x1b, 0, u32::MAX, 23);
        cpu.eip = 0x1000 + call.len() as u32;
        cpu.registers.esp = 0x9008;
        cpu.instruction_count = 1;
        check_one(
            engine,
            SegmentProfile::Segmented16,
            &return_code,
            &image,
            &[SegmentResolution::new(&table, Segment::Cs, 0x1b)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn calls_into_sixteen_bit_code_return_through_newly_resolved_caller_cs() {
    round_trip(Engine::Wasmtime);
}

fn return_frame_width(engine: Engine) {
    for word in [false, true] {
        let code = ret(word, None);
        let mut image = image_with_stack(&code, 0x9000);
        // A dword CALL frame is not a word RET frame: word RET takes selector
        // zero from EIP's high word. The converse reads the sentinel as CS.
        let saved = if word {
            &[7, 0x10, 0, 0, 0x1b, 0, 0xa5, 0xa5][..]
        } else {
            &[6, 0x10, 0x1b, 0, 0xa5, 0xa5, 0xa5, 0xa5][..]
        };
        image.data(0x8000, saved);
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &[],
            Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::GeneralProtection {
                    error: if word { 0 } else { 0xa5a4 },
                },
            },
        );
    }
}

#[test]
fn return_operand_size_selects_the_frame_layout_independently_of_the_caller() {
    return_frame_width(Engine::Wasmtime);
}

fn saved_fallthrough(engine: Engine) {
    for word in [false, true] {
        let code = immediate(word, 0x200, 0x27);
        let mut image = image_with_stack(&[], 0x9008);
        image.cpu.eip = 0x1234_fffc;
        image.map(0x1234f, 0x3000, false);
        image.map(0x12350, 0x5000, false);
        image.data(0x3ffc, &code[..4]);
        image.data(0x5000, &code[4..]);
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
        cpu.eip = 0x200;
        cpu.registers.esp = if word { 0x9004 } else { 0x9000 };
        cpu.instruction_count = 0;
        let ram: &[(u32, &[u8])] = if word {
            &[(0x8004, &[2, 0, 0x1b, 0])]
        } else {
            &[(0x8000, &[3, 0, 0x35, 0x12, 0x1b, 0])]
        };
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &[SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)],
            Step {
                cpu,
                ram,
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn call_saves_the_complete_fallthrough_at_operand_width() {
    saved_fallthrough(Engine::Wasmtime);
}

fn dispatch_boundary(engine: Engine) {
    for returning in [false, true] {
        let instruction = if returning {
            ret(false, None)
        } else {
            immediate(false, 0x200, 0x27)
        };
        let code = [&instruction[..], &[0x62]].concat();
        let mut image = image_with_stack(&code, if returning { 0x9000 } else { 0x9008 });
        image.cpu.segments.cs.limit = 0x1000 + instruction.len() as u32 - 1;
        image.data(0x8000, &pointer(false, 0x200, 0x27));
        let mut input = image.input();
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27));
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
        cpu.eip = 0x200;
        cpu.registers.esp = if returning { 0x9008 } else { 0x9000 };
        cpu.instruction_count = 0;
        let saved = pointer(false, 0x1000 + instruction.len() as u32, 0x1b);
        let ram = if returning {
            vec![]
        } else {
            vec![(0x8000, saved.as_slice())]
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &ram,
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
        // The transfer has retired even though no page backs its destination.
        image.cpu = cpu;
        if !returning {
            image.data(0x8000, &saved);
        }
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
                        address: 0xc200,
                        error: 0x10
                    }
                }]
            )
        );
    }
}

#[test]
fn transfers_end_the_old_block_and_destination_paging_belongs_to_the_next_fetch() {
    dispatch_boundary(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_call_return_dispatch_and_round_trip() {
    round_trip(Engine::V8);
    return_frame_width(Engine::V8);
    saved_fallthrough(Engine::V8);
    dispatch_boundary(Engine::V8);
}
