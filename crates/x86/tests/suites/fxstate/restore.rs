//! FXRSTOR reconstructs tags and replaces state only after validation succeeds.

use super::*;

fn layouts(engine: Engine, frontend: Frontend) {
    for profile in PROFILES {
        let restore = instruction(profile, true, 0x4010);
        let code = [restore.as_slice(), &[0x9b]].concat();
        let mut image = image(&code, profile);
        // FXRSTOR must proceed even when the old state has an unmasked exception.
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.cpu.x87.status = status(0xa881);
        let mut loaded = retire(image.cpu, restore.len());
        for (index, bits) in [
            (0, 0),
            (0, 0x8000),
            (0x8000_0000_0000_0001, 0x3fff),
            (0x8000_0000_0000_0000, 0x7fff),
            (0xc123_0000_0000_0abc, 0x7fff),
            (0x1111_2222_3333_4444, 0x4000),
            (1, 0),
            (0x0123_4567_89ab_cdef, 0xffff),
        ]
        .into_iter()
        .enumerate()
        {
            write_register_bits(&mut loaded, index, bits);
        }
        set_control(&mut loaded.x87.control, 0x035f);
        loaded.x87.status = status(0xa8a0);
        loaded.x87.tag_word = 0xea85;
        loaded.x87.opcode = 0x0654;
        loaded.x87.instruction_offset = if sixteen_bit(profile) {
            0x5678
        } else {
            0x1234_5678
        };
        loaded.x87.instruction_selector = 0x1357;
        loaded.x87.data_offset = if sixteen_bit(profile) {
            0xcdef
        } else {
            0x89ab_cdef
        };
        loaded.x87.data_selector = 0x2468;
        loaded.simd.mxcsr = 0x0041; // An unmasked SIMD flag is loaded without delivery.
        for (index, xmm) in loaded.simd.xmm.iter_mut().enumerate() {
            *xmm = std::array::from_fn(|byte| 0xf0 - index as u8 * 17 - byte as u8);
        }
        let bytes = saved_bytes(
            &loaded,
            [
                0x2820_035f, // ES/B are recomputed from the restored flags and masks.
                0xfe54_a57f, // Reserved bits and byte 5 are ignored; R7 is empty.
                0x1234_5678,
                0xabcd_1357,
                0x89ab_cdef,
                0xbeef_2468,
                0x0041,
                0, // The image's MXCSR_MASK does not restrict supported bits.
            ],
            [5, 6, 7, 0, 1, 2, 3, 4],
        );
        image.data(0x8010, &bytes);
        image.map(4, 0x8000, false);
        ImageSequences::new(engine, frontend, profile).check(
            &format!("FXRSTOR {profile:?} restores physical tags, logical payloads and SIMD state"),
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: loaded,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }
}

fn mmx_tags_and_pending(engine: Engine, frontend: Frontend) {
    for (control, input_status, output_status, occupied, tags) in [
        (0x037f, 0x8081_u16, 0x0001, 0xff, 0xaaaa),
        (0x037e, 0x0001, 0x8081, 0xff, 0xaaaa),
        (0x037e, 0x8080, 0x0000, 0x55, 0xeeee),
    ] {
        let restore = instruction(FLAT32, true, 0x4010);
        let code = [restore.as_slice(), &[0x9b]].concat();
        let mut image = image(&code, FLAT32);
        let mut loaded = retire(image.cpu, restore.len());
        set_control(&mut loaded.x87.control, control);
        loaded.x87.status = status(output_status);
        loaded.x87.tag_word = tags;
        loaded.simd.mxcsr = 0x1f80;
        for (index, significand) in [0, 1, 0x8000_0000_0000_0000, u64::MAX, 2, 3, 4, 5]
            .into_iter()
            .enumerate()
        {
            write_register_bits(&mut loaded, index, (significand, 0xffff));
        }
        let bytes = saved_bytes(
            &loaded,
            [
                u32::from(control) | u32::from(input_status) << 16,
                0x0654_0000 | occupied,
                0x1234_5678,
                0x17,
                0x89ab_cdef,
                0x27,
                0x1f80,
                0xffff_ffff,
            ],
            [0, 1, 2, 3, 4, 5, 6, 7],
        );
        image.data(0x8010, &bytes);
        let wait = if output_status & 0x80 == 0 {
            dispatch(retire(loaded, 1))
        } else {
            Step {
                cpu: loaded,
                ram: &[],
                exit: Exit::FloatingPoint,
            }
        };
        ImageSequences::new(engine, frontend, FLAT32).check(
            "FXRSTOR classifies occupied MMX payloads as special and recomputes ES/B",
            &code,
            &image,
            &[dispatch(loaded), wait],
        );
    }
}

fn restore_then_pop(engine: Engine, frontend: Frontend) {
    let restore = instruction(FLAT32, true, 0x4010);
    let code = [
        &[0xd9, 0xe8][..],         // FLD1
        &[0x66, 0x0f, 0xef, 0xdb], // PXOR xmm3, xmm3
        &restore,
        &[0xdb, 0x3d, 0x10, 0x43, 0, 0], // FSTP tbyte [4310]
    ]
    .concat();
    let mut image = image(&code, FLAT32);
    image.cpu.x87.status = status(0x1800);
    image.cpu.x87.tag_word = 0xffff;
    let mut source = image.cpu;
    source.x87.status = status(0x3800);
    source.x87.tag_word = 0x4300; // R4 is empty, R7 is zero, the rest are normal.
    source.simd.mxcsr = 0x5fc5;
    write_register_bits(&mut source, 7, (0, 0x8000));
    for index in 0..7 {
        write_register_bits(
            &mut source,
            index,
            (0x8000_0000_0000_0000 + index as u64, 0x3fff),
        );
    }
    let bytes = saved_bytes(
        &source,
        [
            0x3800_037f,
            0x0654_00ef,
            0x1234_5678,
            0x17,
            0x89ab_cdef,
            0x27,
            0x5fc5,
            0xffff,
        ],
        [7, 0, 1, 2, 3, 4, 5, 6],
    );
    image.data(0x8010, &bytes);
    let mut pushed = complete_x87(image.cpu, 2, 0x01e8);
    pushed.x87.status = status(0x1000);
    pushed.x87.tag_word = 0xffcf;
    write_register_bits(&mut pushed, 2, (0x8000_0000_0000_0000, 0x3fff));
    let mut dirty = retire(pushed, 4);
    dirty.simd.xmm[3] = [0; 16];
    let mut loaded = retire(dirty, restore.len());
    loaded.x87 = source.x87;
    loaded.simd = source.simd;
    let mut popped = complete_x87(loaded, 6, 0x033d);
    popped.x87.data_offset = 0x4310;
    popped.x87.data_selector = 0x23;
    popped.x87.status = status(0);
    popped.x87.tag_word = 0xc300;
    let zero = real80((0, 0x8000));
    ImageSequences::new(engine, frontend, FLAT32).check(
        "FXRSTOR replaces retained values and empty payloads before a following pop",
        &code,
        &image,
        &[
            dispatch(pushed),
            dispatch(dirty),
            dispatch(loaded),
            Step {
                cpu: popped,
                ram: &[(0x8310, &zero)],
                exit: Exit::Dispatch(popped.eip),
            },
        ],
    );
}

fn invalid_mxcsr(engine: Engine, frontend: Frontend) {
    for mxcsr in [0x0001_1f80, 0x8000_1f80] {
        let restore = instruction(FLAT32, true, 0x4010);
        let code = [&[0xd9, 0xe8][..], &[0x66, 0x0f, 0xef, 0xdb], &restore].concat();
        let mut image = image(&code, FLAT32);
        image.cpu.x87.status = status(0x1800);
        image.cpu.x87.tag_word = 0xffff;
        let bytes = saved_bytes(
            &image.cpu,
            [0x3800_037f, 0x0654_00ff, 0, 0, 0, 0, mxcsr, 0xffff_ffff],
            [7, 0, 1, 2, 3, 4, 5, 6],
        );
        image.data(0x8010, &bytes);
        let mut pushed = complete_x87(image.cpu, 2, 0x01e8);
        pushed.x87.status = status(0x1000);
        pushed.x87.tag_word = 0xffcf;
        write_register_bits(&mut pushed, 2, (0x8000_0000_0000_0000, 0x3fff));
        let mut dirty = retire(pushed, 4);
        dirty.simd.xmm[3] = [0; 16];
        ImageSequences::new(engine, frontend, FLAT32).check(
            "invalid MXCSR preserves earlier x87/XMM writes without partially restoring state",
            &code,
            &image,
            &[
                dispatch(pushed),
                dispatch(dirty),
                Step {
                    cpu: dirty,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                },
            ],
        );
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    for (profile, address, exit) in [
        (FLAT32, 0x4011, Exit::GeneralProtection { error: 0 }),
        (
            FLAT32,
            0x4e10,
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented32),
            0xff00,
            Exit::GeneralProtection { error: 0 },
        ),
        (
            ExecutionProfile::Real16,
            0xff00,
            Exit::GeneralProtection { error: 0 },
        ),
    ] {
        let code = instruction(profile, true, address);
        let mut image = image(&code, profile);
        if profile != FLAT32 {
            image.cpu.segments.ds.limit = 0xffff;
        }
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.cpu.x87.status = status(0xa881);
        ImageSequences::new(engine, frontend, profile).check(
            "FXRSTOR checks alignment and the complete span before MXCSR or state writes",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
}

test_frontends!(fxrstor_layouts, layouts);
test_frontends!(fxrstor_mmx_tags_pending, mmx_tags_and_pending);
test_frontends!(fxrstor_then_pop, restore_then_pop);
test_frontends!(fxrstor_invalid_mxcsr, invalid_mxcsr);
test_frontends!(fxrstor_faults, faults);

#[test]
fn fxrstor_requires_memory_and_no_opcode_prefix() {
    use wasm86_x86::{compile_block_from_bytes, BlockError};
    for code in [
        &[0x0f, 0xae, 0xc8][..],
        &[0x66, 0x0f, 0xae, 0x08],
        &[0xf2, 0x0f, 0xae, 0x08],
        &[0xf3, 0x0f, 0xae, 0x08],
        &[0xf0, 0x0f, 0xae, 0x08],
    ] {
        assert!(matches!(
            compile_block_from_bytes(0x1000, code, 1),
            Err(BlockError::UnsupportedInstruction {
                address: 0x1000,
                ..
            })
        ));
    }
}
