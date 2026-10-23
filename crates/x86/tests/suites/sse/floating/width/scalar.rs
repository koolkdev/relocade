//! Scalar native narrowing admits only its low source and preserves upper bits.

use super::*;
use crate::support::execution::CompiledExecution;
use relocade_x86::{Compiler, ExecutionProfile, Segments};

fn check(case: ScalarCase, restart: bool, profile: ExecutionProfile) {
    let code = [0x90, 0xf2, 0x0f, 0x5a, 0xc0]; // NOP; CVTSD2SS XMM0,XMM0
    let mut image = floating_image(&code, case.controls);
    if profile == ExecutionProfile::Real16 {
        image.cpu.segments = Segments::real_mode();
    }
    lane(&mut image.cpu.simd.xmm[0], true, 0, case.source);
    lane(&mut image.cpu.simd.xmm[0], true, 1, SNAN[1]);
    let prior = retire(image.cpu, 1);
    let fault = case.raised & !(case.controls >> 7) != 0;
    let mut cpu = if fault { prior } else { retire(prior, 4) };
    cpu.simd.mxcsr |= case.raised;
    if !fault {
        lane(&mut cpu.simd.xmm[0], false, 0, case.result);
    }
    let wanted = Step {
        cpu,
        ram: &[],
        exit: if fault {
            Exit::SimdFloatingPoint
        } else {
            Exit::Dispatch(cpu.eip)
        },
    };
    for observed in [false, true] {
        let compiler = Compiler::new(profile);
        let compiler = if observed {
            compiler.specialize_on_cpu(&image.cpu)
        } else {
            compiler
        };
        let compiled = compiler.compile_block(0x1000, &code, 2).unwrap();
        let checks = CompiledExecution::new(&compiled);
        let name = format!(
            "source={:x}, MXCSR={:x}, profile={profile:?}, observed={observed}",
            case.source, case.controls
        );
        if restart {
            checks.check_restart(&name, &image, &[dispatch(prior)], &wanted);
        } else {
            checks.check_native(&name, &image, &wanted);
        }
    }
}

fn native() {
    for (source, results) in [
        (
            0x3ff0_0000_1000_0000,
            [0x3f80_0000, 0x3f80_0000, 0x3f80_0001, 0x3f80_0000],
        ),
        (
            0xbff0_0000_1000_0000,
            [0xbf80_0000, 0xbf80_0001, 0xbf80_0000, 0xbf80_0000],
        ),
    ] {
        for (rounding, result) in results.into_iter().enumerate() {
            for controls in [0x1f80, 0x1fa0, 0x0fa0] {
                check(
                    ScalarCase {
                        source,
                        result,
                        controls: controls | (rounding as u32) << 13,
                        raised: 32,
                    },
                    false,
                    SegmentProfile::Flat32.into(),
                );
            }
        }
    }
    // DAZ-normalized inputs are exact zeros, even with precision unmasked.
    check(
        ScalarCase {
            source: 1,
            result: 0,
            controls: 0x0fc0,
            raised: 0,
        },
        false,
        ExecutionProfile::Real16,
    );
}

fn restart() {
    for (source, result, raised) in [
        (0x3810_0000_0000_0000, 0x0080_0000, 0), // minimum binary32 normal
        (0x3800_0000_0000_0000, 0x0040_0000, 0), // exact binary32 subnormal
        (0x47ef_ffff_f000_0000, 0x7f80_0000, 40), // overflow
        (0xfff0_008a_c000_0123, 0xffc0_0456, 1), // signaling payload
    ] {
        check(
            ScalarCase {
                source,
                result,
                controls: 0x1f80,
                raised,
            },
            true,
            SegmentProfile::Flat32.into(),
        );
    }
}

#[test]
fn scalar_narrow_native_and_restart() {
    native();
    restart();
}
