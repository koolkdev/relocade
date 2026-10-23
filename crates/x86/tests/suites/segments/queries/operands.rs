use super::*;
use crate::SegmentProfile;

#[test]
fn register_queries_honor_destination_width_and_preserve_aliases_on_failure() {
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor {
            available: true,
            limit: SegmentLimit::pages(0x12345).unwrap(),
            ..descriptor(0, SegmentDefaultSize::Bits32)
        },
    );
    for (profile, code, destination, source, value) in [
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x0f, 0x02, 0xc8][..],
            Gpr32::Ecx,
            Gpr32::Eax,
            0x00d0_f300,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x0f, 0x02, 0xff][..],
            Gpr32::Edi,
            Gpr32::Edi,
            0xabcd_f300,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x66, 0x0f, 0x03, 0xf6][..],
            Gpr32::Esi,
            Gpr32::Esi,
            0x1234_5fff,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x66, 0x0f, 0x03, 0xd9][..],
            Gpr32::Ebx,
            Gpr32::Ecx,
            0xcafe_5fff,
        ),
    ] {
        for selector in [0x27, 0x33] {
            let mut image = image(code, profile);
            image.cpu.registers[destination] = 0xcafe_4321;
            image.cpu.registers[source] = 0xabcd_0000 | u32::from(selector);
            let success = selector == 0x27;
            image.cpu.flags.bytes.zf = u8::from(!success);
            let mut cpu = completed(&image, code.len(), success);
            if success {
                cpu.registers[destination] = value;
            }
            Scenario::new(profile, code, &image)
                .query(
                    &[SegmentQuery::new(&tables, selector)],
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip),
                    },
                )
                .check();
        }
    }
}

#[test]
fn memory_selectors_are_words_and_honor_overrides_and_split_pages() {
    let mut tables = DescriptorTables::default();
    tables.insert(0xf327, descriptor(0, SegmentDefaultSize::Bits32));
    let profile = ExecutionProfile::Protected(SegmentProfile::Segmented32);
    for operand_override in [false, true] {
        let word = operand_override;
        for (opcode, value) in [(2, 0x0040_f300), (3, 0xffff)] {
            let mut code = if operand_override { vec![0x66] } else { vec![] };
            code.extend([0x0f, opcode, 3]); // LAR/LSL (E)AX,[EBX].
            let mut image = image(&code, profile);
            image.cpu.registers.ebx = 0x4ffe;
            image.cpu.segments.ds.limit = 0x4fff;
            image.map(4, 0x8000, false);
            image.data(0x8ffe, &[0x27, 0xf3]);
            // Only two bytes fit before the segment/page end, even for r32.
            let mut cpu = completed(&image, code.len(), true);
            cpu.registers.eax = if word {
                (cpu.registers.eax & 0xffff_0000) | (value & 0xffff)
            } else {
                value
            };
            Scenario::new(profile, &code, &image)
                .query(
                    &[SegmentQuery::new(&tables, 0xf327)],
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip),
                    },
                )
                .check();
        }
    }
    for (opcode, value) in [(2, 0x0040_f300), (3, 0xffff)] {
        let profile = ExecutionProfile::Protected(SegmentProfile::Segmented32);
        let code = [0x64, 0x67, 0x0f, opcode, 0x42, 0]; // (E)AX,FS:[BP+SI].
        let mut image = image(&code, profile);
        image.cpu.registers.ebp = 0xabcd_fffe;
        image.cpu.registers.esi = 0xdead_0001;
        image.cpu.segments.fs = data(0x8000, 0x10000);
        image.cpu.segments.ss = crate::SegmentRegister::unusable(0);
        image.map(0x17, 0x8000, false);
        image.map(0x18, 0xa000, false);
        image.data(0x8fff, &[0x27]);
        image.data(0xa000, &[0xf3]);
        let mut cpu = completed(&image, code.len(), true);
        cpu.registers.eax = value;
        Scenario::new(profile, &code, &image)
            .query(
                &[SegmentQuery::new(&tables, 0xf327)],
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
            )
            .check();
    }
}

#[test]
fn source_faults_leave_destination_and_pending_flags_intact_before_any_query() {
    for opcode in [2, 3] {
        for stack in [false, true] {
            let code = if stack {
                vec![0x0f, opcode, 4, 0x24]
            } else {
                vec![0x0f, opcode, 3]
            };
            for segment_denied in [false, true] {
                let profile = ExecutionProfile::Protected(SegmentProfile::Segmented32);
                let mut image = image(&code, profile);
                image.cpu.registers.ebx = 0xfff;
                image.cpu.registers.esp = 0xfff;
                image.cpu.flags.status_source.kind = 10;
                image.cpu.flags.status_source.left = 0x7fff_ffff;
                image.cpu.flags.status_source.right = 1;
                let cache = data(0x4000, if segment_denied { 0xfff } else { 0xffff });
                if stack {
                    image.cpu.segments.ss = cache;
                } else {
                    image.cpu.segments.ds = cache;
                }
                image.map(4, 0x8000, false);
                image.data(0x8fff, &[0]);
                let exit = if segment_denied {
                    if stack {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::GeneralProtection { error: 0 }
                    }
                } else {
                    Exit::PageFault {
                        address: 0x5000,
                        error: 0,
                    }
                };
                Scenario::new(profile, &code, &image)
                    .step(Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit,
                    })
                    .check();
            }
        }
    }
}
