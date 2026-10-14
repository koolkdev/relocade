use super::*;
use wasm86_x86::{Compiler, CpuFeature, CpuModel};

fn configured_features(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (feature, instruction, features) in [
        (
            CpuFeature::Cx8,
            &[0x0f, 0xc7, 0x08][..],
            [0x601, 0x800, 0, 0x0788_8011],
        ),
        (
            CpuFeature::Cmov,
            &[0x0f, 0x44, 0xc3][..],
            [0x601, 0x800, 0, 0x0788_0111],
        ),
        (
            CpuFeature::Tsc,
            &[0x0f, 0x31][..],
            [0x601, 0x800, 0, 0x0788_8101],
        ),
        (
            CpuFeature::Fpu,
            &[0xdb, 0xe3][..],
            [0x601, 0x800, 0, 0x0788_8110],
        ),
        (
            CpuFeature::Mmx,
            &[0x0f, 0x77][..],
            [0x601, 0x800, 0, 0x0708_8111],
        ),
        (
            CpuFeature::Fxsr,
            &[0x0f, 0xae, 0x00][..],
            [0x601, 0x800, 0, 0x0688_8111],
        ),
        (
            CpuFeature::Sse,
            &[0x0f, 0x10, 0xc1][..],
            [0x601, 0x800, 0, 0x0588_8111],
        ),
        (
            CpuFeature::Sse2,
            &[0x66, 0x0f, 0x10, 0xc1][..],
            [0x601, 0x800, 0, 0x0388_8111],
        ),
        (
            CpuFeature::Clflush,
            &[0x0f, 0xae, 0x38][..],
            [0x601, 0, 0, 0x0780_8111],
        ),
        // FISTTP is enabled by default, but partial SSE3 is never advertised.
        (CpuFeature::Sse3, &[0xdb, 0x08][..], FEATURES),
    ] {
        let target = Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(feature, false).unwrap())
            .target();
        for (leaf, values) in [(0, VENDOR), (1, features), (u32::MAX, features)] {
            let mut image = Image::new(&[0x0f, 0xa2]);
            image.cpu.registers.eax = leaf;
            let module = match frontend {
                Frontend::Block => blocks.get_with_target(&image.cpu, &[0x0f, 0xa2], 1, target),
                Frontend::Interpreter => TestModule::interpreter_with_target(target),
            };
            let cpu = result(image.cpu, values, 2, 1);
            assert_eq!(
                engine.observe(module, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip)
                    }]
                ),
                "{feature:?}, leaf {leaf:#x}"
            );
        }
        let image = Image::new(instruction);
        let module = match frontend {
            Frontend::Block => blocks.get_with_target(&image.cpu, instruction, 1, target),
            Frontend::Interpreter => TestModule::interpreter_with_target(target),
        };
        image.check_unchanged_exit(
            engine,
            module,
            "the disabled capability rejects execution",
            Exit::InvalidOpcode,
        );
    }

    // Optional capabilities can all be disabled; the identity and CPUID remain available.
    let mut model = CpuModel::default();
    for feature in [
        CpuFeature::Cx8,
        CpuFeature::Cmov,
        CpuFeature::Tsc,
        CpuFeature::Fpu,
        CpuFeature::Mmx,
        CpuFeature::Fxsr,
        CpuFeature::Sse,
        CpuFeature::Sse2,
        CpuFeature::Sse3,
        CpuFeature::Clflush,
    ] {
        model = model.with_feature(feature, false).unwrap();
    }
    let compiler = Compiler::new(SegmentProfile::Flat32).with_cpu_model(model);
    let mut image = Image::new(&[0x0f, 0xa2]);
    image.cpu.registers.eax = 1;
    let module = match frontend {
        Frontend::Block => blocks.get_with_target(&image.cpu, &[0x0f, 0xa2], 1, compiler.target()),
        Frontend::Interpreter => TestModule::interpreter_with_target(compiler.target()),
    };
    let cpu = result(image.cpu, [0x601, 0, 0, 0], 2, 1);
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip)
            }]
        )
    );
}
test_frontends!(
    reported_capabilities_control_instruction_admission,
    configured_features
);
