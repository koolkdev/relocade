use super::*;

fn registers(engine: Engine) {
    let mut tables = DescriptorTables::default();
    tables.insert(0xffff, descriptor(0, SegmentDefaultSize::Bits16));
    for (profile, code, source) in [
        (SegmentProfile::Flat32, &[0x0f, 0x00, 0xe7][..], Gpr32::Edi),
        (
            SegmentProfile::Segmented16,
            &[0x66, 0x0f, 0x00, 0xec][..],
            Gpr32::Esp,
        ),
        (
            SegmentProfile::Segmented16,
            &[0x0f, 0x00, 0xe2][..],
            Gpr32::Edx,
        ),
        (
            SegmentProfile::Segmented32,
            &[0x66, 0x0f, 0x00, 0xe9][..],
            Gpr32::Ecx,
        ),
    ] {
        let mut image = image(code, profile);
        image.cpu.registers[source] = 0xabcd_ffff;
        let cpu = completed(&image, code.len(), true);
        check_one(
            engine,
            profile,
            code,
            &image,
            &[SegmentQuery::new(&tables, 0xffff)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn register_forms_use_low_words_in_both_code_sizes_with_or_without_overrides() {
    registers(Engine::Wasmtime);
}

fn memory_sources(engine: Engine) {
    let mut tables = DescriptorTables::default();
    tables.insert(0xf327, descriptor(0, SegmentDefaultSize::Bits32));
    let profile = SegmentProfile::Segmented32;
    for operand_override in [false, true] {
        for extension in [4, 5] {
            let mut code = if operand_override { vec![0x66] } else { vec![] };
            code.extend([0x0f, 0x00, (extension << 3) | 3]); // [EBX].
            let mut image = image(&code, profile);
            image.cpu.registers.ebx = 0x4ffe;
            image.cpu.segments.ds.limit = 0x4fff;
            image.map(4, 0x8000, false);
            image.data(0x8ffe, &[0x27, 0xf3]);
            // A widened read would touch the unmapped next page.
            let cpu = completed(&image, code.len(), true);
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[SegmentQuery::new(&tables, 0xf327)],
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
    for extension in [4, 5] {
        let profile = SegmentProfile::Segmented32;
        let code = [0x64, 0x67, 0x0f, 0x00, 0x40 | (extension << 3) | 2, 0]; // FS:[BP+SI].
        let mut image = image(&code, profile);
        image.cpu.registers.ebp = 0xabcd_fffe;
        image.cpu.registers.esi = 0xdead_0001;
        image.cpu.segments.fs = data(0x8000, 0x10000);
        image.cpu.segments.ss = crate::StoredSegment::unusable(0);
        image.map(0x17, 0x8000, false);
        image.map(0x18, 0xa000, false);
        image.data(0x8fff, &[0x27]);
        image.data(0xa000, &[0xf3]);
        let cpu = completed(&image, code.len(), true);
        check_one(
            engine,
            profile,
            &code,
            &image,
            &[SegmentQuery::new(&tables, 0xf327)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn selector_reads_are_word_sized_read_only_and_honor_segment_and_address_overrides() {
    memory_sources(Engine::Wasmtime);
}

fn source_faults(engine: Engine) {
    for (code, stack) in [
        (vec![0x0f, 0x00, 0x23], false),
        (vec![0x0f, 0x00, 0x2c, 0x24], true),
    ] {
        for segment_denied in [false, true] {
            let profile = SegmentProfile::Segmented32;
            let mut image = image(&code, profile);
            image.cpu.registers.ebx = 0xfff;
            image.cpu.registers.esp = 0xfff;
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
            // No scripted query proves that neither the host nor ZF was touched.
            check_one(
                engine,
                profile,
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
fn source_segment_and_page_faults_precede_permission_queries_and_flag_changes() {
    source_faults(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_verification_operands_and_source_faults() {
    registers(Engine::V8);
    memory_sources(Engine::V8);
    source_faults(Engine::V8);
}
