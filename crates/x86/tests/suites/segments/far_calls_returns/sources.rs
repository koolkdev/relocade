use super::*;

fn aliased_sources(engine: Engine) {
    for word in [false, true] {
        for override_fs in [false, true] {
            // The default-SS pointer occupies the cells that CALL will overwrite.
            // With FS, the source is elsewhere but implicit stack writes still use SS.
            let mut code = if word { vec![0x66] } else { vec![] };
            if override_fs {
                code.push(0x64);
            }
            code.extend([0xff, 0x5c, 0x24, if word { 0xfc } else { 0xf8 }]);
            let mut image = Image::new(&code);
            image.cpu.segments.cs.selector = 0x1b;
            image.cpu.registers.esp = if word { 0x9004 } else { 0x9008 };
            image.cpu.segments.ss = data(0x4000, 0xffff);
            image.cpu.segments.fs = data(0x6000, 0xffff);
            image.map(0xd, 0x8000, true);
            image.map(0xf, 0x5000, false);
            image.data(0x8000, &[0xa5; 8]);
            image.data(
                if override_fs { 0x5000 } else { 0x8000 },
                &pointer(word, 0x200, 0x27),
            );
            let mut cpu = image.cpu;
            cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
            cpu.eip = 0x200;
            cpu.registers.esp = 0x9000;
            cpu.instruction_count = 0;
            let saved = pointer(word, 0x1000 + code.len() as u32, 0x1b);
            check_one(
                engine,
                SegmentProfile::Segmented32,
                &code,
                &image,
                &[SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)],
                Step {
                    cpu,
                    ram: &[(0x8000, &saved)],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn indirect_calls_read_entry_esp_and_source_bytes_before_pushing_through_ss() {
    aliased_sources(Engine::Wasmtime);
}

fn source_faults(engine: Engine) {
    for word in [false, true] {
        for (stack_source, limit, mapped, exit) in [
            (false, 0xfff, false, Exit::GeneralProtection { error: 0 }),
            (true, 0xfff, false, Exit::StackFault { error: 0 }),
            (
                false,
                0xffff,
                false,
                Exit::PageFault {
                    address: if word { 0x4ffe } else { 0x4ffc },
                    error: 0,
                },
            ),
            (
                false,
                0xffff,
                true,
                Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            ),
        ] {
            let mut code = if word { vec![0x66] } else { vec![] };
            code.extend(if stack_source {
                &[0xff, 0x5d, 0][..]
            } else {
                &[0xff, 0x1b]
            });
            let mut image = Image::new(&code);
            image.cpu.segments.cs.selector = 0x1b;
            image.cpu.registers.ebx = if word { 0xffe } else { 0xffc };
            image.cpu.registers.ebp = image.cpu.registers.ebx;
            image.cpu.registers.esp = 0x9008; // Stack is absent, but source faults win.
            image.cpu.segments[if stack_source {
                Segment::Ss
            } else {
                Segment::Ds
            }] = data(0x4000, limit);
            if mapped {
                image.map(4, 0x8000, false);
            }
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

#[test]
fn the_whole_indirect_pointer_faults_before_resolution_or_return_frame_access() {
    source_faults(Engine::Wasmtime);
}

fn overrides_on_return(engine: Engine) {
    // An unusable FS and address override do not redirect implicit stack accesses.
    let code = [0x64, 0x67, 0xcb];
    let mut image = image_with_stack(&code, 0x9000);
    image.cpu.segments.fs = loaded(0, 0x8000, 0, 0);
    image.data(0x8000, &pointer(false, 0x200, 0x27));
    let mut cpu = image.cpu;
    cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
    cpu.eip = 0x200;
    cpu.registers.esp = 0x9008;
    cpu.instruction_count = 0;
    check_one(
        engine,
        SegmentProfile::Flat32,
        &code,
        &image,
        &[SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

#[test]
fn return_ignores_address_and_segment_overrides_for_its_stack() {
    overrides_on_return(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_call_sources_and_return_prefixes() {
    aliased_sources(Engine::V8);
    source_faults(Engine::V8);
    overrides_on_return(Engine::V8);
}
