use super::*;
use crate::support::machine::Exit;
use crate::SystemSelector;

fn query(selector: u16) -> Query {
    Query::SystemSelector(SystemSelectorQuery::new(SystemSelector::Ldtr, selector))
}

fn registers(engine: Engine) {
    for (profile, code, selector, eax) in [
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x0f, 0x00, 0xc0][..],
            0xf328,
            0xf328,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x66, 0x0f, 0x00, 0xc0][..],
            0xf328,
            0xabcd_f328,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x0f, 0x00, 0xc0][..],
            0xf328,
            0xabcd_f328,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x66, 0x0f, 0x00, 0xc0][..],
            0xf328,
            0xf328,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x0f, 0x00, 0xc0][..],
            0,
            0,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x66, 0x0f, 0x00, 0xc0][..],
            0,
            0xabcd_0000,
        ),
    ] {
        let mut image = image(code, profile);
        image.cpu.registers.eax = 0xabcd_ffff;
        let mut cpu = completed(&image, code.len());
        cpu.registers.eax = eax;
        check_query(
            engine,
            profile,
            code,
            &image,
            Some(query(selector)),
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

fn memory(engine: Engine) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
    let code = [0x0f, 0x00, 0x03]; // SLDT [EBX].
    for offset in [0x4ffe, 0x4fff] {
        let mut image = image(&code, profile);
        image.cpu.registers.ebx = offset;
        image.map(4, 0x8000, true);
        image.data(0x8ffd, &[0xa5; 3]);
        let (cpu, exit) = if offset == 0x4ffe {
            let cpu = completed(&image, code.len());
            (cpu, Exit::Dispatch(cpu.eip))
        } else {
            (
                image.cpu,
                Exit::PageFault {
                    address: 0x5000,
                    error: 2,
                },
            )
        };
        let stored = [(0x8ffe, &[0x28, 0xf3][..])];
        check_query(
            engine,
            profile,
            &code,
            &image,
            Some(query(0xf328)),
            Step {
                cpu,
                ram: if offset == 0x4ffe { &stored } else { &[] },
                exit,
            },
        );
    }
}

fn real_mode(engine: Engine) {
    let profile = ExecutionProfile::Real16;
    for code in [
        &[0x0f, 0x00, 0xc0][..],
        &[0x66, 0x0f, 0x00, 0xc0][..],
        &[0x0f, 0x00, 0x06, 0xff, 0xff][..],
    ] {
        let image = image(code, profile);
        check_query(
            engine,
            profile,
            code,
            &image,
            None,
            Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::InvalidOpcode,
            },
        );
    }
    let mut image = image(&[0x0f, 0x00], profile);
    image.cpu.eip = 0xfffe;
    image.map(0xf, 0x8000, false);
    image.data(0x8ffe, &[0x0f, 0x00]);
    image.check_unchanged_exit(
        engine,
        TestModule::interpreter_with_profile(profile),
        "the missing ModRM fetch faults before SLDT's mode check",
        Exit::GeneralProtection { error: 0 },
    );
}

#[test]
fn selector_and_register_widths_preserve_flags() {
    registers(Engine::Wasmtime);
}

#[test]
fn memory_destination_is_a_word_and_faults_before_writes() {
    memory(Engine::Wasmtime);
}

#[test]
fn real_mode_rejects_only_after_complete_decoding() {
    real_mode(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_selector_stores_and_mode_checks() {
    registers(Engine::V8);
    memory(Engine::V8);
    real_mode(Engine::V8);
}
