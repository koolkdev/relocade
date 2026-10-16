use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{Engine, TestModule},
    },
    Compiler, CpuState, ExecutionProfile, Segment, SegmentProfile, Segments, StoredSegment,
};

fn image(profile: ExecutionProfile, code: &[u8]) -> Image {
    let mut image = Image::new(code);
    if profile == ExecutionProfile::Real16 {
        image.cpu.segments = Segments::real_mode();
    } else if profile == SegmentProfile::Segmented16.into() {
        image.cpu.segments.cs.big = 0;
    }
    image.cpu.instruction_count = 17;
    image.cpu.registers.eax = 0;
    image.cpu.registers.ecx = 0;
    image.cpu.registers.ebx = 0x4000;
    image
}

fn profiles() -> [(ExecutionProfile, bool); 3] {
    [
        (ExecutionProfile::Real16, false),
        (SegmentProfile::Segmented16.into(), false),
        (SegmentProfile::Segmented32.into(), true),
    ]
}

fn dynamic_data_and_stack(engine: Engine) {
    for (profile, wide) in profiles() {
        // MOV AX/EAX,CS:[BX/EBX]; ADD AX/EAX,[BX/EBX]; PUSH AX/EAX; POP CX/ECX.
        let operand = if wide { 3 } else { 7 };
        let code = [0x2e, 0x8b, operand, 0x03, operand, 0x50, 0x59];
        let mut image = image(profile, &code);
        let module = TestModule::new(
            &Compiler::new(profile)
                .specialize_on_cpu(&image.cpu)
                .compile_block(0x1000, &code, 4)
                .unwrap(),
        );
        // These changes happen after compilation and must remain runtime inputs.
        image.cpu.segments.ds = StoredSegment::real_mode(Segment::Ds, 0x1000);
        image.cpu.segments.ss = StoredSegment::real_mode(Segment::Ss, 0x2000);
        image.cpu.registers.esp = 0x7004;
        image.map(4, 0x8000, false);
        image.map(0x14, 0x9000, false);
        image.map(0x27, 0xa000, true);
        image.data(0x8000, &3u32.to_le_bytes());
        image.data(0x9000, &5u32.to_le_bytes());
        // Compare complete flag publication against ordinary execution as well
        // as checking the independently calculated register and progress values.
        let ordinary = TestModule::new(
            &Compiler::new(profile)
                .compile_block(0x1000, &code, 4)
                .unwrap(),
        );
        let actual = engine.observe(&module, &image.input(), 1);
        assert_eq!(
            actual,
            engine.observe(&ordinary, &image.input(), 1),
            "{profile:?}"
        );
        let returned = actual.events.last().unwrap();
        let crate::support::step::Event::Return { snapshot, .. } = returned else {
            panic!("missing return")
        };
        let actual_cpu = CpuState::from_bytes(snapshot.cpu.clone().try_into().unwrap());
        assert_eq!(
            (
                actual_cpu.registers.eax,
                actual_cpu.registers.ecx,
                actual_cpu.registers.esp
            ),
            (8, 8, 0x7004)
        );
        assert_eq!(actual_cpu.segments, image.cpu.segments);
        assert_eq!((actual_cpu.eip, actual_cpu.instruction_count), (0x1007, 21));
    }
}

fn mismatches_restart_at_entry(engine: Engine) {
    for (profile, wide) in profiles() {
        let code = [0x40, 0x89, if wide { 3 } else { 7 }]; // INC; MOV [BX/EBX],AX/EAX.
        let mut initial = image(profile, &code);
        initial.map(4, 0x8000, true);
        initial.data(0x8000, &[0xa5; 4]);
        let module = TestModule::new(
            &Compiler::new(profile)
                .specialize_on_cpu(&initial.cpu)
                .compile_block(0x1000, &code, 2)
                .unwrap(),
        );
        let mut variants = Vec::new();
        if profile == ExecutionProfile::Real16 {
            variants.push(StoredSegment::real_mode(Segment::Cs, 0x100));
        } else {
            let cs = initial.cpu.segments.cs;
            variants.extend([
                StoredSegment { base: 0x8000, ..cs },
                StoredSegment {
                    limit: 0x7fff,
                    ..cs
                },
                StoredSegment {
                    selector: 0x1b,
                    ..cs
                },
                StoredSegment { readable: 0, ..cs },
                StoredSegment { usable: 0, ..cs },
            ]);
        }
        for cs in variants {
            initial.cpu.segments.cs = cs;
            initial.check_unchanged_exit(
                engine,
                &module,
                "CS mismatch precedes all instruction effects",
                Exit::Interpret,
            );
        }
    }
}

fn observed_permissions_still_apply(engine: Engine) {
    let profile = SegmentProfile::Segmented32.into();
    let code = [0x2e, 0x8b, 0x03];
    let mut image = image(profile, &code);
    image.cpu.segments.cs.readable = 0;
    let module = TestModule::new(
        &Compiler::new(profile)
            .specialize_on_cpu(&image.cpu)
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    );
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            }]
        )
    );
}

#[test]
fn cs_specialization_keeps_data_and_stack_dynamic() {
    dynamic_data_and_stack(Engine::Wasmtime);
}
#[test]
fn cs_specialization_mismatches_restart_before_effects() {
    mismatches_restart_at_entry(Engine::Wasmtime);
}
#[test]
fn cs_specialization_preserves_observed_permissions() {
    observed_permissions_still_apply(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_cs_specialization() {
    dynamic_data_and_stack(Engine::V8);
    mismatches_restart_at_entry(Engine::V8);
    observed_permissions_still_apply(Engine::V8);
}

#[test]
fn flat_blocks_use_profile_constants_without_cs_guards() {
    let cpu = CpuState::default();
    let code = [0x2e, 0x8b, 0x03, 0x0e]; // MOV EAX,CS:[EBX]; PUSH CS.
    let ordinary = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 2)
        .unwrap();
    let observed = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&cpu)
        .compile_block(0x1000, &code, 2)
        .unwrap();
    assert_eq!(observed.bytes, ordinary.bytes);
}
