//! x87 environment controls, deferred exception delivery and memory commitment.

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
    x87::status,
};
use wasm86_x86::{compile_block_from_bytes, BlockError, CpuState, SegmentProfile, StoredX87Status};

fn initial_image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.x87.control_word = 0x037f;
    image.cpu.x87.status = status(0x3a20);
    image.cpu.x87.tag_word = 0x5a5a;
    image.cpu.x87.opcode = 0x0654;
    image.cpu.x87.instruction_offset = 0x1234_5678;
    image.cpu.x87.data_offset = 0x89ab_cdef;
    image.cpu.x87.instruction_selector = 0x1b;
    image.cpu.x87.data_selector = 0x23;
    image
}

fn pending(image: &mut Image) {
    image.cpu.x87.control_word = 0x035f;
    image.cpu.x87.status = status(0xbaa0);
}

fn retire(mut cpu: CpuState, bytes: u32) -> CpuState {
    cpu.eip = cpu.eip.wrapping_add(bytes);
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn dispatch(cpu: CpuState) -> Step<'static> {
    Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    }
}

fn initialized(mut cpu: CpuState) -> CpuState {
    cpu.x87.control_word = 0x037f;
    cpu.x87.status = status(0);
    cpu.x87.tag_word = 0xffff;
    cpu.x87.opcode = 0;
    cpu.x87.instruction_offset = 0;
    cpu.x87.data_offset = 0;
    cpu.x87.instruction_selector = 0;
    cpu.x87.data_selector = 0;
    cpu
}

fn reset_and_clear(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (name, code) in [
        ("FNINIT", &[0xdb, 0xe3, 0x9b][..]),
        ("FNCLEX", &[0xdb, 0xe2, 0x9b][..]),
    ] {
        let mut image = initial_image(code);
        pending(&mut image);
        image.cpu.x87.status = status(0xffff);
        let mut cpu = retire(image.cpu, 2);
        if code[1] == 0xe3 {
            cpu = initialized(cpu);
        } else {
            // All exception/SF/ES/B bits clear. Retaining undefined condition
            // codes is this implementation's policy, not an Intel guarantee.
            cpu.x87.status = status(0x7f00);
        }
        checks.check(
            name,
            code,
            &image,
            &[dispatch(cpu), dispatch(retire(cpu, 1))],
        );
    }
}

fn no_wait_stores(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (name, store, stored) in [
        (
            "FNSTCW m16",
            &[0xd9, 0x3d, 1, 0x40, 0, 0][..],
            Some([0x5f, 0x03]),
        ),
        (
            "66 FNSTSW m16",
            &[0x66, 0xdd, 0x3d, 1, 0x40, 0, 0][..],
            Some([0xa0, 0xba]),
        ),
        ("FNSTSW AX", &[0xdf, 0xe0][..], None),
        ("66 FNSTSW AX", &[0x66, 0xdf, 0xe0][..], None),
    ] {
        let code = [store, &[0x9b]].concat();
        let mut image = initial_image(&code);
        pending(&mut image);
        image.cpu.registers.eax = 0xabcd_1234;
        image.map(4, 0x8000, true);
        image.data(0x8000, &[0x11, 0x22, 0x33, 0x44]);
        let mut cpu = retire(image.cpu, store.len() as u32);
        let ram = stored.as_ref().map(|bytes| (0x8001, bytes.as_slice()));
        if stored.is_none() {
            cpu.registers.eax = 0xabcd_baa0;
        }
        let ram = ram.as_slice();
        checks.check(
            name,
            &code,
            &image,
            &[
                Step {
                    cpu,
                    ram,
                    exit: Exit::Dispatch(cpu.eip),
                },
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }
}

fn status_field_packing(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xdf, 0xe0];
    for (name, fields, word) in [
        (
            "FNSTSW packs C1/C3 and independent B without ES",
            StoredX87Status {
                exception_flags: 0xff,
                top: 0xfa,
                c0: 0xfe,
                c1: 0xa5,
                c2: 0x80,
                c3: 0xff,
                error_summary: 0x80,
                busy: 0x01,
            },
            0xd27f,
        ),
        (
            "FNSTSW packs C0/C2 and independent ES without B",
            StoredX87Status {
                exception_flags: 0xa0,
                top: 0xfd,
                c0: 0x81,
                c1: 0xa4,
                c2: 0xff,
                c3: 0x82,
                error_summary: 0xff,
                busy: 0xfe,
            },
            0x2da0,
        ),
    ] {
        let mut image = initial_image(&code);
        image.cpu.x87.status = fields;
        image.cpu.registers.eax = 0xabcd_1234;
        let mut observed = retire(image.cpu, 2);
        observed.registers.eax = 0xabcd_0000 | word;
        // Packing observes only architectural low bits. The backing bytes,
        // including unrelated upper bits and an independent ES/B pair, survive.
        checks.check(name, &code, &image, &[dispatch(observed)]);
    }
}

fn load_control_and_pending(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // FLDCW m16; FNSTSW AX; FWAIT.
    let code = [0xd9, 0x2d, 1, 0x40, 0, 0, 0xdf, 0xe0, 0x9b];
    for flag in [1_u16, 2, 4, 8, 16, 32] {
        let mut image = initial_image(&code);
        image.cpu.x87.status = status(0x3a00 | flag);
        image.map(4, 0x8000, false);
        let control = 0x037f & !flag;
        image.data(0x8001, &control.to_le_bytes());
        let mut loaded = retire(image.cpu, 6);
        loaded.x87.control_word = control;
        loaded.x87.status = status(0xba80 | flag);
        let mut observed = retire(loaded, 2);
        observed.registers.eax = 0x1111_0000 | u32::from(0xba80 | flag);
        checks.check(
            &format!("FLDCW unmasks sticky flag {flag:#x} after its own completion"),
            &code,
            &image,
            &[
                dispatch(loaded),
                dispatch(observed),
                Step {
                    cpu: observed,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }

    let code = [0x66, 0xd9, 0x2d, 1, 0x40, 0, 0, 0x9b];
    let mut image = initial_image(&code);
    image.map(4, 0x8000, false);
    image.data(0x8001, &[0x7f, 0x0a]);
    let mut loaded = retire(image.cpu, 7);
    loaded.x87.control_word = 0x0a7f;
    checks.check(
        "FLDCW changes PC/RC without rounding payloads or unmasking PE",
        &code,
        &image,
        &[dispatch(loaded), dispatch(retire(loaded, 1))],
    );

    let code = [0xd9, 0x2d, 0, 0x40, 0, 0];
    let mut image = initial_image(&code);
    pending(&mut image);
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0x7f, 0x03]);
    checks.check(
        "a pending exception prevents FLDCW from masking it",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

fn waiting_boundaries(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0x9b, 0xdb, 0xe3];
    let image = initial_image(&code);
    let waited = retire(image.cpu, 1);
    checks.check(
        "FWAIT retires separately before FNINIT",
        &code,
        &image,
        &[dispatch(waited), dispatch(initialized(retire(waited, 2)))],
    );
    let mut image = image;
    pending(&mut image);
    checks.check(
        "pending FINIT spelling faults at FWAIT before FNINIT",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

fn repeated_delivery(engine: Engine) {
    let mut image = initial_image(&[0x9b]);
    pending(&mut image);
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 2),
        expected(
            &image,
            &[
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::FloatingPoint
                },
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::FloatingPoint
                },
            ],
        ),
        "reporting #MF does not clear pending state",
    );
}

fn memory_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (name, opcode, modrm, error) in [
        ("FLDCW", 0xd9, 0x2d, 0),
        ("FNSTCW", 0xd9, 0x3d, 2),
        ("FNSTSW", 0xdd, 0x3d, 2),
    ] {
        let code = [0xdb, 0xe2, opcode, modrm, 0xff, 0x4f, 0, 0];
        let mut image = initial_image(&code);
        pending(&mut image);
        image.map(4, 0x8000, true);
        image.data(0x8fff, &[0xcc]);
        let mut cleared = retire(image.cpu, 2);
        cleared.x87.status = status(0x3a00);
        checks.check(
            &format!("faulting split {name} preserves earlier FNCLEX and the first byte"),
            &code,
            &image,
            &[
                dispatch(cleared),
                Step {
                    cpu: cleared,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error,
                    },
                },
            ],
        );
    }

    let code = [0xd9, 0x2d, 0, 0x40, 0, 0];
    let mut image = initial_image(&code);
    image.cpu.segments.ds.limit = 0x4000;
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented32);
    checks.check(
        "the complete two-byte control operand must fit the segment",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}

fn rejected_forms(engine: Engine) {
    for code in [
        &[0xd9, 0xe8][..], // FLDCW's /5 is memory-only; this register form is FLD1.
        &[0xdd, 0xf8][..],
        &[0xdf, 0xe1][..],
        &[0xdb, 0x23][..], // The FNINIT byte does not select a memory operand.
        &[0xf0, 0xdb, 0xe3][..],
        &[0xf0, 0xd9, 0x3d][..],
    ] {
        let origin = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: origin,
                opcode: code[0]
            }),
        );
        let mut image = initial_image(&[]);
        image.cpu.eip = origin;
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("unselected x87 form {code:02x?}"),
            Exit::Other(0x0008_0000_0000_0000 | u64::from(code[0]) << 32 | u64::from(origin)),
        );
    }
}

test_frontends!(controls_reset_and_clear_without_waiting, reset_and_clear);
test_frontends!(
    no_wait_stores_preserve_pending_state_and_use_fixed_word_destinations,
    no_wait_stores
);
test_frontends!(status_fields_pack_at_observation, status_field_packing);
test_frontends!(
    control_loads_establish_pending_exceptions_for_a_later_wait,
    load_control_and_pending
);
test_frontends!(
    waiting_is_a_separate_restart_and_retirement_boundary,
    waiting_boundaries
);
test_frontends!(
    memory_faults_publish_earlier_controls_without_partial_commitment,
    memory_faults
);

#[test]
fn repeated_delivery_retains_the_pending_exception() {
    repeated_delivery(Engine::Wasmtime);
}

#[test]
fn controls_reject_other_modrm_forms_and_lock_prefixes() {
    rejected_forms(Engine::Wasmtime);
}

#[test]
fn controls_consume_exact_encodings_and_wait_has_its_own_length() {
    for code in [
        &[0x9b][..],
        &[0xdb, 0xe2][..],
        &[0xdb, 0xe3][..],
        &[0xdf, 0xe0][..],
        &[0x66, 0xdf, 0xe0][..],
        &[0xd9, 0x2d, 0, 0x40, 0, 0][..],
        &[0xd9, 0x3d, 0, 0x40, 0, 0][..],
        &[0xdd, 0x3d, 0, 0x40, 0, 0][..],
    ] {
        check_length(code);
    }
    assert_eq!(
        compile_block_from_bytes(0x1000, &[0x9b], 1).unwrap().bytes,
        compile_block_from_bytes(0x1000, &[0x9b, 0xdb, 0xe3], 1)
            .unwrap()
            .bytes,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_repeated_delivery_and_rejected_forms() {
    repeated_delivery(Engine::V8);
    rejected_forms(Engine::V8);
}
