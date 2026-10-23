//! Memory fences preserve architectural state and remain observable compiler effects.

#[path = "memory_control/cache.rs"]
mod cache;

use crate::support::wasm::{function_bodies, memory_import_index};
use crate::support::{
    encoding::check_unsupported_at_page_end,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
};
use relocade_x86::{compile_block_from_bytes, ExecutionProfile, SegmentProfile, Segments};
use wasmparser::Operator;

fn fences(frontend: Frontend) {
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        let mut checks = ImageSequences::new(frontend, profile);
        for modrm in 0xe8..=0xff {
            let code = [0x0f, 0xae, modrm];
            let mut image = Image::new(&code);
            if profile == ExecutionProfile::Real16 {
                image.cpu.segments = Segments::real_mode();
            }
            image.cpu.x87.status.error_summary = 1;
            image.cpu.simd.mxcsr = 0x9fc5;
            let mut cpu = image.cpu;
            cpu.eip += 3;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            checks.check(
                "fences ignore ModRM.r/m and do not consult floating-point state",
                &code,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

test_frontends!(fences_preserve_state, fences);

#[test]
fn fences_require_register_encodings_and_reject_lock() {
    // The /7 memory form is CLFLUSH, covered by the cache-control tests.
    for modrm in [0x28, 0x30] {
        check_unsupported_at_page_end(&[0x0f, 0xae, modrm], 0x0f);
    }
    for modrm in [0xe8, 0xf0, 0xf8] {
        check_unsupported_at_page_end(&[0xf0, 0x0f, 0xae, modrm], 0xf0);
    }
}

#[test]
fn explicit_fences_remain_between_memory_effects() {
    // Each store/fence/load sequence observes the same guest address. The fence
    // must prevent forwarding the stored value across its synchronization point.
    for modrm in [0xe8, 0xf0, 0xf8] {
        let code = [0x89, 0x03, 0x0f, 0xae, modrm, 0x8b, 0x13];
        let module = compile_block_from_bytes(0x1000, &code, 3).unwrap();
        let guest = memory_import_index(&module.bytes, "relocade", "guest");
        let mut fence_count = 0;
        let mut ordered_guest_accesses = false;
        for body in function_bodies(&module.bytes) {
            let mut stored = false;
            let mut fenced_store = false;
            for operator in body.get_operators_reader().unwrap() {
                match operator.unwrap() {
                    Operator::I32Store { memarg } if Some(memarg.memory) == guest => {
                        stored = true;
                    }
                    Operator::AtomicFence => {
                        fence_count += 1;
                        fenced_store = stored;
                    }
                    Operator::I32Load { memarg } if Some(memarg.memory) == guest => {
                        ordered_guest_accesses |= fenced_store;
                    }
                    _ => {}
                }
            }
        }
        assert!(ordered_guest_accesses);
        assert_eq!(fence_count, 1);
    }
}
