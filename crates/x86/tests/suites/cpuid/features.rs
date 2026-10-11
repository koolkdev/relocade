use super::*;
use wasm86_x86::{Compiler, CpuFeature, CpuModel};

fn configured_features(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (feature, instruction, features) in [
        (
            CpuFeature::Cx8,
            &[0x0f, 0xc7, 0x08][..],
            [0x601, 0, 0, 0x8010],
        ),
        (
            CpuFeature::Cmov,
            &[0x0f, 0x44, 0xc3][..],
            [0x601, 0, 0, 0x0110],
        ),
        (CpuFeature::Tsc, &[0x0f, 0x31][..], [0x601, 0, 0, 0x8100]),
    ] {
        let target = Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(feature, false))
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
            "the unreported capability also rejects execution",
            Exit::InvalidOpcode,
        );
    }

    // Optional capabilities can all be disabled; the identity and CPUID remain available.
    let model = CpuModel::default()
        .with_feature(CpuFeature::Cx8, false)
        .with_feature(CpuFeature::Cmov, false)
        .with_feature(CpuFeature::Tsc, false);
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
