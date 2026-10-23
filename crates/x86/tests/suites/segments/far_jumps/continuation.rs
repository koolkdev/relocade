use super::*;
use crate::SegmentProfile;
use relocade_x86::SegmentRegister;

#[test]
fn reloading_cs_changes_the_next_fetch_base_and_decode_defaults() {
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
        cpu.segments.cs = loaded(
            0x1b,
            0x9000,
            0xffff,
            SegmentRegister {
                big: u8::from(!next_word),
                readable: 0,
                ..SegmentRegister::flat_code32(0)
            },
        );
        cpu.eip = 0x200;
        cpu.instruction_count = 0;
        Scenario::new(
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &code,
            &image,
        )
        .resolve(
            &[SegmentResolution::new(&tables, Segment::Cs, 0x1b)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        )
        .check();
        // Dispatch admits a fresh entry whose defaults and fetch base match the new CS.
        image.cpu = cpu;
        cpu.registers.eax = if next_word { 0x1111_5678 } else { 0x9234_5678 };
        cpu.eip += next_code.len() as u32;
        cpu.instruction_count = 1;
        Scenario::new(
            if next_word {
                ExecutionProfile::Protected(SegmentProfile::Segmented16)
            } else {
                ExecutionProfile::Protected(SegmentProfile::Segmented32)
            },
            next_code,
            &image,
        )
        .step(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        })
        .check();
    }
}

#[test]
fn destination_paging_happens_on_the_next_fetch_after_the_jump_retires() {
    let code = immediate(false, 0x200, 0x27);
    let mut image = Image::new(&code);
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0x9000, 0xffff, SegmentDefaultSize::Bits32));
    let mut cpu = image.cpu;
    cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, SegmentRegister::flat_code32(0));
    cpu.eip = 0x200;
    cpu.instruction_count = 0;
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        &code,
        &image,
    )
    .resolve(
        &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    )
    .check();
    image.cpu = cpu;
    assert_eq!(
        TestModule::interpreter_with_profile(ExecutionProfile::Protected(
            SegmentProfile::Segmented32
        ))
        .observe(&image.input(), 1),
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
fn both_far_jump_forms_end_the_block_without_fetching_old_fallthrough() {
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
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, SegmentRegister::flat_code32(0));
        cpu.eip = 0x200;
        cpu.instruction_count = 0;
        let mut blocks = BlockModules::default();
        Scenario::new(
            ExecutionProfile::Protected(SegmentProfile::Segmented32),
            &code,
            &image,
        )
        .resolve(
            &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        )
        .check_with_block(blocks.get(
            &image.cpu,
            &code,
            2,
            ExecutionProfile::Protected(SegmentProfile::Segmented32),
        ));
    }
}
