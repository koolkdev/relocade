//! Extension availability and admission precede operand effects.

use super::*;

fn disabled_extensions(frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (feature, forms) in [
        (
            CpuFeature::Fpu,
            &[
                &[0xd9, 0x00][..], // FLD m32
                &[0x9b],           // FWAIT
                &[0xda, 0xc1],     // FCMOVB
                &[0xdb, 0xf1],     // FCOMI
            ][..],
        ),
        (
            CpuFeature::Cmov,
            &[
                &[0xda, 0xc1][..],
                &[0xdb, 0xf1], // FCMOVB, FCOMI
            ][..],
        ),
        (
            CpuFeature::Mmx,
            &[
                &[0x0f, 0x6f, 0x00][..], // MOVQ mm0, m64
                &[0x0f, 0xfc, 0xc1],     // PADDB mm0, mm1
            ][..],
        ),
        (
            CpuFeature::Fxsr,
            &[
                &[0x0f, 0xae, 0x00][..],
                &[0x0f, 0xae, 0x08], // FXSAVE, FXRSTOR
            ][..],
        ),
        (
            CpuFeature::Sse,
            &[
                &[0xf3, 0x0f, 0x10, 0x00][..], // MOVSS from memory
                &[0x0f, 0x2a, 0xc1],           // CVTPI2PS
                &[0x0f, 0x2c, 0xc1],           // CVTTPS2PI
                &[0x0f, 0xe0, 0xc1],           // PAVGB mm0, mm1
                &[0x0f, 0xe7, 0x00],           // MOVNTQ
                &[0x0f, 0xae, 0x10],
                &[0x0f, 0xae, 0x18],       // LDMXCSR, STMXCSR
                &[0x0f, 0xae, 0xf8],       // SFENCE
                &[0x0f, 0x18, 0x00],       // PREFETCHNTA
                &[0x0f, 0xc6, 0xc1, 0x1b], // SHUFPS with an immediate
            ][..],
        ),
        (
            CpuFeature::Sse2,
            &[
                &[0xf3, 0x0f, 0x6f, 0x00][..], // MOVDQU from memory
                &[0xf2, 0x0f, 0x58, 0xc1],     // ADDSD
                &[0x0f, 0xd4, 0xc1],           // PADDQ mm0, mm1
                &[0xf2, 0x0f, 0xd6, 0xc1],     // MOVDQ2Q
                &[0x66, 0x0f, 0x2a, 0xc1],     // CVTPI2PD
                &[0x0f, 0xc3, 0x00],           // MOVNTI
                &[0x0f, 0xae, 0xe8],
                &[0x0f, 0xae, 0xf0], // LFENCE, MFENCE
            ][..],
        ),
        (CpuFeature::Clflush, &[&[0x0f, 0xae, 0x38][..]][..]),
    ] {
        let target = Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(feature, false))
            .target();
        for &instruction in forms {
            let code = [instruction, &[0x0f]].concat();
            let mut image = Image::new(&code);
            image.cpu.x87.status.error_summary = 1;
            image.cpu.flags.status_source.kind = 0;
            image.cpu.flags.bytes.cf = 0; // Untaken FCMOVB still requires both features.
            let module = match frontend {
                Frontend::Block => blocks.get_with_target(&image.cpu, &code, 8, target),
                Frontend::Interpreter => TestModule::interpreter_with_target(target),
            };
            image.check_unchanged_exit(
                module,
                &format!("disabled {feature:?}: {instruction:02x?}"),
                Exit::InvalidOpcode,
            );
        }
    }
}
test_frontends!(disabled_extensions_preserve_all_state, disabled_extensions);
