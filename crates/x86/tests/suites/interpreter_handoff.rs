//! Guard admission and the exact state published at an interpreter handoff.

#[path = "interpreter_handoff/linked.rs"]
mod linked;

use crate::support::{
    machine::{expected, Exit, Step},
    step::{Argument, Engine, Event, Outcome, TestModule},
    x87::stack_image,
};
use wasm86_x86::{compile_block_from_bytes, compile_block_from_bytes_with_profile, SegmentProfile};

fn guard_admission(engine: Engine) {
    // Literal encodings cover normal, signed zeros, infinity, quiet/signaling
    // NaNs and subnormal values. The final case occupies the push destination.
    let single = [
        (0x3f80_0000_u64, 0xffff, false),
        (0, 0xffff, false),
        (0x8000_0000, 0xffff, false),
        (0x7f80_0000, 0xffff, false),
        (0x7fc0_0001, 0xffff, false),
        (0x7f80_0001, 0xffff, true),
        (1, 0xffff, true),
        (0x3f80_0000, 0x3fff, true),
    ];
    let double = [
        (0x3ff0_0000_0000_0000_u64, 0xffff, false),
        (0, 0xffff, false),
        (0x8000_0000_0000_0000, 0xffff, false),
        (0x7ff0_0000_0000_0000, 0xffff, false),
        (0x7ff8_0000_0000_0001, 0xffff, false),
        (0x7ff0_0000_0000_0001, 0xffff, true),
        (1, 0xffff, true),
        (0x3ff0_0000_0000_0000, 0x3fff, true),
    ];
    for (opcode, width, cases) in [(0xd9, 4, single), (0xdd, 8, double)] {
        let code = [opcode, 0x05, 0, 0x40, 0, 0];
        let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        for (bits, tags, handoff) in cases {
            let mut image = stack_image(&code, 0, tags);
            image.map(4, 0x8000, false);
            image.data(0x8000, &bits.to_le_bytes()[..width]);
            let actual = engine.observe(&module, &image.input(), 1);
            if handoff {
                assert_eq!(
                    actual,
                    expected(
                        &image,
                        &[Step {
                            cpu: image.cpu,
                            ram: &[],
                            exit: Exit::Interpret,
                        }]
                    )
                );
            } else {
                // Full conversion semantics belong to the x87 load suite. These
                // values must reach dispatch through the compiled path.
                assert!(
                    matches!(actual.events.as_slice(), [
                    Event::Dispatch { eip: 0x1006, .. },
                    Event::Return { outcome: Outcome::Returned(values), .. },
                ] if values == &[Argument::I64(i64::MIN)]),
                    "{opcode:02x} {bits:016x}"
                );
                assert!(actual.guest_unchanged && actual.machine_unchanged);
            }
        }
    }
}

fn faults_before_handoff(engine: Engine) {
    let code = [0xd9, 0x05, 0, 0x40, 0, 0];
    let module = TestModule::new(
        &compile_block_from_bytes_with_profile(0x1000, &code, 1, SegmentProfile::Segmented32)
            .unwrap(),
    );
    for (pending, segment_denied, exit) in [
        (
            false,
            false,
            Exit::PageFault {
                address: 0x4000,
                error: 0,
            },
        ),
        (true, false, Exit::FloatingPoint),
        (false, true, Exit::GeneralProtection { error: 0 }),
    ] {
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status.error_summary = u8::from(pending);
        if segment_denied {
            image.cpu.segments.ds.limit = 0x3fff;
        }
        image.check_unchanged_exit(engine, &module, "fault before guard", exit);
    }
}

#[test]
fn only_guarded_jit_entries_require_the_interpreter_import() {
    fn imports_interpreter(bytes: &[u8]) -> bool {
        wasmparser::Parser::new(0)
            .parse_all(bytes)
            .any(|section| match section.unwrap() {
                wasmparser::Payload::ImportSection(imports) => imports.into_iter().any(|import| {
                    let import = import.unwrap();
                    import.module == "wasm86" && import.name == "interpret"
                }),
                _ => false,
            })
    }
    let load = compile_block_from_bytes(0x1000, &[0xd9, 0x05, 0, 0x40, 0, 0], 1).unwrap();
    assert!(imports_interpreter(&load.bytes));
    let plain = compile_block_from_bytes(0x1000, &[0x90], 1).unwrap();
    assert!(!imports_interpreter(&plain.bytes));
    assert!(!imports_interpreter(TestModule::interpreter().bytes()));
    assert!(!imports_interpreter(linked::interpreter_run().bytes()));
}

#[test]
fn binary_load_guards_admit_common_values_and_reject_exceptional_pushes() {
    guard_admission(Engine::Wasmtime);
}

#[test]
fn pending_segment_and_page_faults_precede_interpreter_handoff() {
    faults_before_handoff(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_handoff_boundaries() {
    guard_admission(Engine::V8);
    faults_before_handoff(Engine::V8);
}
