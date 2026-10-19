//! Selected capabilities affect admission before operand access in both frontends.

#[path = "cpu_features/extensions.rs"]
mod extensions;

use std::sync::OnceLock;

use crate::support::{
    blocks::BlockModules,
    execution::{test_frontends, Frontend},
    machine::{expected, Exit, Image, Step},
    step::TestModule,
};
use wasm86_x86::{
    BlockError, Compiler, CpuFeature, CpuModel, ExecutionProfile, SegmentProfile, Segments,
};

fn disabled_forms(frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        for (feature, forms) in [
            (
                CpuFeature::Cmov,
                &[&[0x0f, 0x44, 0xc3][..], &[0x66, 0x0f, 0x44, 0x00]][..],
            ),
            (
                CpuFeature::Cx8,
                &[&[0x0f, 0xc7, 0x08][..], &[0xf0, 0x0f, 0xc7, 0x08]][..],
            ),
            (
                CpuFeature::Tsc,
                &[&[0x0f, 0x31][..], &[0x66, 0x0f, 0x31]][..],
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
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false))
}

#[test]
fn disabled_instruction_publishes_only_prior_work_and_stops_the_run() {
    let code = [0xbb, 7, 0, 0, 0, 0x0f, 0x44, 0x03, 0x0f];
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
            module.observe(&image.input(), 1),
            wanted,
            "{}",
            module.entry
        );
    }
}

#[test]
fn interpreter_fetch_fault_precedes_disabled_feature_fault() {
    let module = TestModule::interpreter_with_target(restricted_compiler().target());
    for (code, exit) in [
        (
            &[0x0f, 0x44][..],
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (&[0x0f, 0x44, 0x00][..], Exit::InvalidOpcode),
    ] {
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(module, "feature admission follows complete fetch", exit);
    }
}

#[test]
fn disabled_forms_remain_decodable_and_unrelated_forms_keep_their_code() {
    assert!(matches!(
        restricted_compiler().compile_block(0x1000, &[0x0f, 0x44], 1),
        Err(BlockError::TruncatedInstruction {
            address: 0x1000,
            available: 2
        }),
    ));
    use CpuFeature::*;
    let forms = [
        (&[Cmov][..], &[0x0f, 0x44, 0xc3][..]), // CMOVE
        (&[Cx8], &[0x0f, 0xc7, 0x08]),          // CMPXCHG8B
        (&[Tsc], &[0x0f, 0x31]),                // RDTSC
        (&[Fpu], &[0xdb, 0xe3]),                // FNINIT
        (&[Fpu, Cmov], &[0xda, 0xc1]),          // FCMOVB
        (&[Fpu, Cmov], &[0xdb, 0xf1]),          // FCOMI
        (&[Mmx], &[0x0f, 0x6f, 0xc1]),          // MOVQ mm0, mm1
        (&[Mmx], &[0x0f, 0x77]),                // EMMS
        (&[Fxsr], &[0x0f, 0xae, 0x00]),         // FXSAVE
        (&[Fxsr], &[0x0f, 0xae, 0x08]),         // FXRSTOR
        (&[Sse], &[0x0f, 0x10, 0xc1]),          // MOVUPS
        (&[Sse], &[0x0f, 0xe0, 0xc1]),          // PAVGB mm0, mm1
        (&[Sse], &[0x0f, 0x2a, 0xc1]),          // CVTPI2PS
        (&[Sse], &[0x0f, 0xae, 0xf8]),          // SFENCE
        (&[Sse2], &[0x66, 0x0f, 0x10, 0xc1]),   // MOVUPD
        (&[Sse2], &[0x0f, 0xd4, 0xc1]),         // PADDQ mm0, mm1
        (&[Sse2], &[0xf2, 0x0f, 0xd6, 0xc1]),   // MOVDQ2Q
        (&[Sse2], &[0x66, 0x0f, 0x2a, 0xc1]),   // CVTPI2PD
        (&[Sse2], &[0x0f, 0xc3, 0x00]),         // MOVNTI
        (&[Sse2], &[0x0f, 0xae, 0xe8]),         // LFENCE
        (&[Clflush], &[0x0f, 0xae, 0x38]),      // CLFLUSH
        (&[], &[0x89, 0xd8]),                   // MOV
        (&[], &[0xf3, 0x90]),                   // PAUSE also executes as REP NOP without SSE2.
    ];
    let ordinary: Vec<_> = forms
        .iter()
        .map(|(_, code)| {
            Compiler::new(SegmentProfile::Flat32)
                .compile_block(0x1000, code, 1)
                .unwrap()
                .bytes
        })
        .collect();
    for disabled in [Cx8, Cmov, Tsc, Fpu, Mmx, Fxsr, Sse, Sse2, Clflush] {
        let compiler = Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(disabled, false));
        for ((requirements, code), bytes) in forms.iter().zip(&ordinary) {
            let configured = compiler.compile_block(0x1000, code, 1).unwrap();
            assert_eq!(
                *bytes == configured.bytes,
                !requirements.contains(&disabled),
                "disabling {disabled:?}, instruction {code:02x?}"
            );
        }
    }
}
