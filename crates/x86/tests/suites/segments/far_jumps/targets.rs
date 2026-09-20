use super::*;

fn widths_and_profiles(engine: Engine) {
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        for override_size in [false, true] {
            let word = (profile == SegmentProfile::Segmented16) != override_size;
            for indirect in [false, true] {
                for next_word in [false, true] {
                    let payload = pointer(word, 0x9234_5678, 0xf324);
                    let mut code = vec![];
                    if override_size {
                        code.push(0x66);
                    }
                    if indirect {
                        code.extend(if profile == SegmentProfile::Segmented16 {
                            [0xff, 0x28] // [BX+SI].
                        } else {
                            [0xff, 0x2b] // [EBX].
                        });
                    } else {
                        code.push(0xea);
                        code.extend(&payload);
                    }
                    let mut image = Image::new(&code);
                    code_defaults(&mut image, profile);
                    image.cpu.registers.ebx = 0x4000;
                    image.cpu.registers.esi = 0;
                    image.map(4, 0x8000, false);
                    image.data(0x8000, &payload);
                    let mut tables = DescriptorTables::default();
                    tables.insert(
                        0xf324,
                        descriptor(
                            0x9000,
                            u32::MAX,
                            if next_word {
                                SegmentDefaultSize::Bits16
                            } else {
                                SegmentDefaultSize::Bits32
                            },
                        ),
                    );
                    let mut cpu = image.cpu;
                    cpu.segments.cs =
                        loaded(0xf327, 0x9000, u32::MAX, if next_word { 7 } else { 23 });
                    // CS.D sets subsequent defaults; it does not narrow this transfer.
                    cpu.eip = if word { 0x5678 } else { 0x9234_5678 };
                    cpu.instruction_count = 0;
                    check_one(
                        engine,
                        profile,
                        &code,
                        &image,
                        &[SegmentResolution::new(&tables, Segment::Cs, 0xf324)],
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
}

#[test]
fn both_forms_use_operand_size_for_the_target_and_preserve_all_other_state() {
    widths_and_profiles(Engine::Wasmtime);
}

fn accepted_descriptors(engine: Engine) {
    for selector_slot in [0x18, 0x04] {
        // GDT and the valid LDT index zero.
        for rpl in 0..4 {
            for (readable, conforming, dpl) in [
                (false, false, PrivilegeLevel::Ring3),
                (true, true, PrivilegeLevel::Ring0),
            ] {
                let selector = selector_slot | rpl;
                let code = immediate(false, 0x200, selector);
                let image = Image::new(&code);
                let mut tables = DescriptorTables::default();
                tables.insert(
                    selector,
                    SegmentDescriptor {
                        kind: SegmentDescriptorKind::Code {
                            readable,
                            conforming,
                        },
                        dpl,
                        ..descriptor(0x9000, 0x200, SegmentDefaultSize::Bits32)
                    },
                );
                let mut cpu = image.cpu;
                cpu.segments.cs = loaded(
                    selector_slot | 3,
                    0x9000,
                    0x200,
                    if readable { 23 } else { 19 },
                );
                cpu.eip = 0x200;
                cpu.instruction_count = 0;
                check_one(
                    engine,
                    SegmentProfile::Flat32,
                    &code,
                    &image,
                    &[SegmentResolution::new(&tables, Segment::Cs, selector)],
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
fn execute_only_and_conforming_code_accept_table_selection_and_normalize_cs_rpl() {
    accepted_descriptors(Engine::Wasmtime);
}

fn target_limits(engine: Engine) {
    for profile in [SegmentProfile::Flat32, SegmentProfile::Segmented32] {
        for (old_limit, new_limit, target) in [
            (u32::MAX, 0, 0),
            (u32::MAX, 0x200, 0x200),
            (u32::MAX, 0x200, 0x201),
            (0x1006, 0x4000, 0x3000),
            (0x1006, 0x200, 0x201),
        ] {
            if profile == SegmentProfile::Flat32 && old_limit != u32::MAX {
                continue;
            }
            let code = immediate(false, target, 0x27);
            let mut image = Image::new(&code);
            image.cpu.segments.cs.limit = old_limit;
            let mut tables = DescriptorTables::default();
            tables.insert(
                0x27,
                descriptor(0x9000, new_limit, SegmentDefaultSize::Bits32),
            );
            let mut cpu = image.cpu;
            let exit = if target <= new_limit {
                cpu.segments.cs = loaded(0x27, 0x9000, new_limit, 23);
                cpu.eip = target;
                cpu.instruction_count = 0;
                Exit::Dispatch(target)
            } else {
                Exit::GeneralProtection { error: 0 }
            };
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
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
fn the_new_inclusive_code_limit_applies_even_when_the_incoming_profile_is_flat() {
    target_limits(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_jump_targets_widths_and_descriptors() {
    widths_and_profiles(Engine::V8);
    accepted_descriptors(Engine::V8);
    target_limits(Engine::V8);
}
