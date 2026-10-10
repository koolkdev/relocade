//! Selected capabilities affect admission before operand access in both frontends.

use std::sync::OnceLock;

use crate::support::{
    blocks::BlockModules,
    execution::{test_frontends, Frontend},
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    BlockError, Compiler, CpuFeature, CpuModel, ExecutionProfile, SegmentProfile, Segments,
};

fn disabled_forms(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        for (feature, forms) in [
            (
                CpuFeature::Popcnt,
                &[
                    &[0xf3, 0x0f, 0xb8, 0xc3][..],
                    &[0x66, 0xf3, 0x0f, 0xb8, 0x00],
                ][..],
            ),
            (
                CpuFeature::Cmov,
                &[&[0x0f, 0x44, 0xc3][..], &[0x66, 0x0f, 0x44, 0x00]][..],
            ),
            (
                CpuFeature::Cx8,
                &[&[0x0f, 0xc7, 0x08][..], &[0xf0, 0x0f, 0xc7, 0x08]][..],
            ),
        ] {
            let model = CpuModel::default().with_feature(feature, false);
            let target = Compiler::new(profile).with_cpu_model(model).target();
            for &instruction in forms {
                // The incomplete successor must not enter the snapshot boundary.
                let code = [instruction, &[0x0f]].concat();
                let mut image = Image::new(&code);
                if profile == ExecutionProfile::Real16 {
                    image.cpu.segments = Segments::real_mode();
                }
                image.cpu.flags.status_source.kind = 0;
                image.cpu.flags.bytes.zf = 0; // Even an untaken CMOVE requires CMOV.
                let module = match frontend {
                    Frontend::Block => blocks.get_with_target(&image.cpu, &code, 8, target),
                    Frontend::Interpreter => TestModule::interpreter_with_target(target),
                };
                image.check_unchanged_exit(
                    engine,
                    module,
                    "disabled capability precedes register, flag and unmapped-memory access",
                    Exit::InvalidOpcode,
                );
            }
        }
    }
}
test_frontends!(disabled_forms_preserve_state, disabled_forms);

fn restricted_compiler() -> Compiler {
    Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Popcnt, false))
}

fn prior_work(engine: Engine) {
    let code = [0xbb, 7, 0, 0, 0, 0xf3, 0x0f, 0xb8, 0x03, 0x0f];
    let image = Image::new(&code);
    let mut cpu = image.cpu;
    cpu.registers.ebx = 7;
    cpu.eip += 5;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    let wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::InvalidOpcode,
        }],
    );
    let block = TestModule::new(
        &restricted_compiler()
            .compile_block(0x1000, &code, 8)
            .unwrap(),
    );
    static RUN: OnceLock<TestModule> = OnceLock::new();
    let run =
        RUN.get_or_init(|| TestModule::new(&restricted_compiler().compile_interpreter().unwrap()));
    for module in [&block, run] {
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            wanted,
            "{}",
            module.entry
        );
    }
}

#[test]
fn disabled_instruction_publishes_only_prior_work_and_stops_the_run() {
    prior_work(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_disabled_instruction_publishes_only_prior_work_and_stops_the_run() {
    prior_work(Engine::V8);
}

fn complete_fetch(engine: Engine) {
    let module = TestModule::interpreter_with_target(restricted_compiler().target());
    for (code, exit) in [
        (
            &[0xf3, 0x0f, 0xb8][..],
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (&[0xf3, 0x0f, 0xb8, 0x00][..], Exit::InvalidOpcode),
    ] {
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            module,
            "feature admission follows complete fetch",
            exit,
        );
    }
}

#[test]
fn interpreter_fetch_fault_precedes_disabled_feature_fault() {
    complete_fetch(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interpreter_fetch_fault_precedes_disabled_feature_fault() {
    complete_fetch(Engine::V8);
}

#[test]
fn disabled_forms_remain_decodable_and_unrelated_forms_keep_their_code() {
    assert!(matches!(
        restricted_compiler().compile_block(0x1000, &[0xf3, 0x0f, 0xb8], 1),
        Err(BlockError::TruncatedInstruction {
            address: 0x1000,
            available: 3
        }),
    ));
    let forms = [
        (CpuFeature::Popcnt, &[0xf3, 0x0f, 0xb8, 0xc3][..]),
        (CpuFeature::Cmov, &[0x0f, 0x44, 0xc3][..]),
        (CpuFeature::Cx8, &[0x0f, 0xc7, 0x08][..]),
    ];
    for (disabled, _) in forms {
        let compiler = Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(disabled, false));
        for (required, code) in forms {
            let ordinary = Compiler::new(SegmentProfile::Flat32)
                .compile_block(0x1000, code, 1)
                .unwrap();
            let configured = compiler.compile_block(0x1000, code, 1).unwrap();
            assert_eq!(ordinary.bytes == configured.bytes, required != disabled);
        }
        // Base instructions and partial extension support have no selectable complete feature.
        for code in [
            &[0x89, 0xd8][..],
            &[0xdb, 0xe3][..],
            &[0x0f, 0x10, 0xc1][..],
        ] {
            let ordinary = Compiler::new(SegmentProfile::Flat32)
                .compile_block(0x1000, code, 1)
                .unwrap();
            assert_eq!(
                compiler.compile_block(0x1000, code, 1).unwrap().bytes,
                ordinary.bytes
            );
        }
    }
}
