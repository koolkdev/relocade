//! FXSAVE preserves live state, including pending exceptions and MMX aliases.

use super::*;

fn layouts(engine: Engine, frontend: Frontend) {
    for profile in PROFILES {
        let save = instruction(profile, false, 0x4010);
        let code = [save.as_slice(), &[0x9b]].concat();
        let mut image = image(&code, profile);
        set_control(&mut image.cpu.x87.control, 0x035f);
        image.cpu.x87.status = status(0xa8a0);
        let bytes = saved_bytes(
            &image.cpu,
            [
                0xa8a0_035f,
                0x0654_008f, // Occupied physical registers R0..R3 and R7.
                if sixteen_bit(profile) {
                    0x5678
                } else {
                    0x1234_5678
                },
                0x0017,
                if sixteen_bit(profile) {
                    0xcdef
                } else {
                    0x89ab_cdef
                },
                0x0027,
                0x9fc5,
                0xffff,
            ],
            [5, 6, 7, 0, 1, 2, 3, 4],
        );
        let saved = retire(image.cpu, save.len());
        ImageSequences::new(engine, frontend, profile).check(
            &format!("FXSAVE {profile:?}: one layout and no reset or wait"),
            &code,
            &image,
            &[
                Step {
                    cpu: saved,
                    ram: &[(0x8010, &bytes)],
                    exit: Exit::Dispatch(saved.eip),
                },
                Step {
                    cpu: saved,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }
}

fn mmx_and_xmm(engine: Engine, frontend: Frontend) {
    let save = instruction(FLAT32, false, 0x4010);
    let save_empty = instruction(FLAT32, false, 0x4210);
    let code = [
        &[0x0f, 0xef, 0xc0][..],   // PXOR mm0, mm0
        &[0x66, 0x0f, 0xef, 0xdb], // PXOR xmm3, xmm3
        &save,
        &[0x0f, 0x77], // EMMS
        &save_empty,
    ]
    .concat();
    let mut image = image(&code, FLAT32);
    image.data(0x8210, &[0xa5; 512]);
    let mut mmx = retire(image.cpu, 3);
    mmx.x87.status.top = 0;
    mmx.x87.tag_word = 0;
    write_register_bits(&mut mmx, 0, (0, 0xffff));
    let mut xmm = retire(mmx, 4);
    xmm.simd.xmm[3] = [0; 16];
    let saved = retire(xmm, save.len());
    let header = [
        0x037f,
        0x0654_00ff,
        0x1234_5678,
        0x17,
        0x89ab_cdef,
        0x27,
        0x9fc5,
        0xffff,
    ];
    let bytes = saved_bytes(&xmm, header, [0, 1, 2, 3, 4, 5, 6, 7]);
    let mut emptied = retire(saved, 2);
    emptied.x87.tag_word = 0xffff;
    let saved_empty = retire(emptied, save_empty.len());
    let mut empty_header = header;
    empty_header[1] = 0x0654_0000;
    let empty_bytes = saved_bytes(&emptied, empty_header, [0, 1, 2, 3, 4, 5, 6, 7]);
    ImageSequences::new(engine, frontend, FLAT32).check(
        "FXSAVE observes retained MMX/XMM writes and keeps raw payloads after EMMS",
        &code,
        &image,
        &[
            dispatch(mmx),
            dispatch(xmm),
            Step {
                cpu: saved,
                ram: &[(0x8010, &bytes)],
                exit: Exit::Dispatch(saved.eip),
            },
            dispatch(emptied),
            Step {
                cpu: saved_empty,
                ram: &[(0x8210, &empty_bytes)],
                exit: Exit::Dispatch(saved_empty.eip),
            },
        ],
    );
}

fn suppressed_push(engine: Engine, frontend: Frontend) {
    let save = instruction(FLAT32, false, 0x4010);
    let code = [&[0xd9, 0xe8][..], &save].concat();
    let mut image = image(&code, FLAT32);
    image.cpu.x87.status = status(0x0800);
    image.cpu.x87.tag_word = 0;
    set_control(&mut image.cpu.x87.control, 0x037e);
    let mut faulted = complete_x87(image.cpu, 2, 0x01e8);
    faulted.x87.status = status(0x8ac1);
    let bytes = saved_bytes(
        &image.cpu,
        [
            0x8ac1_037e,
            0x01e8_00ff,
            0x1000,
            0x1b,
            0x89ab_cdef,
            0x27,
            0x9fc5,
            0xffff,
        ],
        [1, 2, 3, 4, 5, 6, 7, 0],
    );
    let saved = retire(faulted, save.len());
    ImageSequences::new(engine, frontend, FLAT32).check(
        "FXSAVE retains a suppressed push and uses actual TOP without delivering its exception",
        &code,
        &image,
        &[
            dispatch(faulted),
            Step {
                cpu: saved,
                ram: &[(0x8010, &bytes)],
                exit: Exit::Dispatch(saved.eip),
            },
        ],
    );
}

fn faults(engine: Engine, frontend: Frontend) {
    for (address, read_only, expected) in [
        (0x4011, false, Exit::GeneralProtection { error: 0 }),
        (
            0x4e10,
            false,
            Exit::PageFault {
                address: 0x5000,
                error: 2,
            },
        ),
        (
            0x4010,
            true,
            Exit::PageFault {
                address: 0x4010,
                error: 3,
            },
        ),
    ] {
        let code = instruction(FLAT32, false, address);
        let mut image = image(&code, FLAT32);
        if read_only {
            image.map(4, 0x8000, false);
        }
        image.data(0x8e10, &[0xa5; 512]);
        ImageSequences::new(engine, frontend, FLAT32).check(
            "FXSAVE validates 16-byte alignment and the whole 512-byte operand before effects",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: expected,
            }],
        );
    }
    for profile in [
        ExecutionProfile::Protected(SegmentProfile::Segmented32),
        ExecutionProfile::Real16,
    ] {
        let code = instruction(profile, false, 0xff00);
        let mut image = image(&code, profile);
        image.cpu.segments.ds.limit = 0xffff;
        ImageSequences::new(engine, frontend, profile).check(
            "FXSAVE rejects an image spanning the segment limit",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            }],
        );
    }
}

test_frontends!(fxsave_layouts, layouts);
test_frontends!(fxsave_mmx_xmm, mmx_and_xmm);
test_frontends!(fxsave_suppressed_push, suppressed_push);
test_frontends!(fxsave_faults, faults);

#[test]
fn fxsave_requires_memory_and_no_opcode_prefix() {
    use wasm86_x86::{compile_block_from_bytes, BlockError};
    for code in [
        &[0x0f, 0xae, 0xc0][..],
        &[0x66, 0x0f, 0xae, 0x00],
        &[0xf2, 0x0f, 0xae, 0x00],
        &[0xf3, 0x0f, 0xae, 0x00],
        &[0xf0, 0x0f, 0xae, 0x00],
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
