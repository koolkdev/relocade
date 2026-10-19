//! Restored MMX aliases, XMM lanes and MXCSR controls replace retained state.

use super::*;

fn restored_transfers(frontend: Frontend) {
    let restore = instruction(FLAT32, true, 0x4010);
    let save = instruction(FLAT32, false, 0x4310);
    let code = [
        &[0x0f, 0xef, 0xc9][..],   // PXOR mm1, mm1
        &[0x66, 0x0f, 0xef, 0xdb], // PXOR xmm3, xmm3
        &restore,
        &[0xf3, 0x0f, 0xd6, 0xc1], // MOVQ2DQ xmm0, mm1
        &[0xf2, 0x0f, 0xd6, 0xd3], // MOVDQ2Q mm2, xmm3
        &[0xf3, 0x0f, 0x10, 0xdc], // MOVSS xmm3, xmm4
        &save,
    ]
    .concat();
    let mut sequences = ImageSequences::new(frontend, FLAT32);
    for top in 0..8_u8 {
        let mut image = image(&code, FLAT32);
        image.data(0x8310, &[0xa5; 512]);
        let mut source = image.cpu;
        source.x87.status = status(u16::from(top) << 11);
        source.x87.tag_word = 0xeeee; // Odd physical slots are empty, including MM1.
        source.simd.mxcsr = 0x5f80;
        for index in 0..8 {
            write_register_bits(
                &mut source,
                index,
                (0x8123_4567_89ab_cd00 + index as u64, 0xffff),
            );
            source.simd.xmm[index] =
                std::array::from_fn(|byte| 0xf0 - index as u8 * 17 - byte as u8);
        }
        let header = [
            0x037f | (u32::from(top) << 27),
            0x0654_0055,
            0x1234_5678,
            0x17,
            0x89ab_cdef,
            0x27,
            0x5f80,
            0xffff,
        ];
        let order = std::array::from_fn(|index| (usize::from(top) + index) & 7);
        let bytes = saved_bytes(&source, header, order);
        image.data(0x8010, &bytes);

        let mut mmx_dirty = retire(image.cpu, 3);
        write_register_bits(&mut mmx_dirty, 1, (0, 0xffff));
        mmx_dirty.x87.status.top = 0;
        mmx_dirty.x87.tag_word = 0;
        let mut xmm_dirty = retire(mmx_dirty, 4);
        xmm_dirty.simd.xmm[3] = [0; 16];
        let mut loaded = retire(xmm_dirty, restore.len());
        loaded.x87 = source.x87;
        loaded.simd = source.simd;

        // MM1 names physical R1 regardless of the restored TOP or empty tag.
        let mut to_xmm = retire(loaded, 4);
        to_xmm.simd.xmm[0] = [0; 16];
        to_xmm.simd.xmm[0][..8].copy_from_slice(&source.x87.registers[1].significand.to_le_bytes());
        to_xmm.x87.status.top = 0;
        to_xmm.x87.tag_word = 0;
        let mut to_mmx = retire(to_xmm, 4);
        let qword = u64::from_le_bytes(source.simd.xmm[3][..8].try_into().unwrap());
        write_register_bits(&mut to_mmx, 2, (qword, 0xffff));
        // A register MOVSS preserves the other twelve restored destination bytes.
        let mut scalar = retire(to_mmx, 4);
        scalar.simd.xmm[3][..4].copy_from_slice(&source.simd.xmm[4][..4]);
        let result = saved_bytes(
            &scalar,
            [
                0x037f,
                0x0654_00ff,
                0x1234_5678,
                0x17,
                0x89ab_cdef,
                0x27,
                0x5f80,
                0xffff,
            ],
            [0, 1, 2, 3, 4, 5, 6, 7],
        );
        let saved = retire(scalar, save.len());
        sequences.check(
            &format!("restored MMX/XMM transfers at TOP {top} replace retained values"),
            &code,
            &image,
            &[
                dispatch(mmx_dirty),
                dispatch(xmm_dirty),
                dispatch(loaded),
                dispatch(to_xmm),
                dispatch(to_mmx),
                dispatch(scalar),
                Step {
                    cpu: saved,
                    ram: &[(0x8310, &result)],
                    exit: Exit::Dispatch(saved.eip),
                },
            ],
        );
    }
}

fn failed_restore(frontend: Frontend) {
    let restore = instruction(FLAT32, true, 0x4010);
    let code = [
        &[0xf2, 0x0f, 0xd6, 0xfb][..], // MOVDQ2Q mm7, xmm3
        &[0x66, 0x0f, 0xef, 0xdb],     // PXOR xmm3, xmm3
        &restore,
    ]
    .concat();
    let mut image = image(&code, FLAT32);
    let bytes = saved_bytes(
        &image.cpu,
        [
            0x3800_037f,
            0x0654_00ff,
            0,
            0,
            0,
            0,
            0x8000_1f80,
            0xffff_ffff,
        ],
        [7, 0, 1, 2, 3, 4, 5, 6],
    );
    image.data(0x8010, &bytes);
    let mut mmx_dirty = retire(image.cpu, 4);
    let qword = u64::from_le_bytes(image.cpu.simd.xmm[3][..8].try_into().unwrap());
    write_register_bits(&mut mmx_dirty, 7, (qword, 0xffff));
    mmx_dirty.x87.status.top = 0;
    mmx_dirty.x87.tag_word = 0;
    let mut xmm_dirty = retire(mmx_dirty, 4);
    xmm_dirty.simd.xmm[3] = [0; 16];
    ImageSequences::new(frontend, FLAT32).check(
        "faulting FXRSTOR preserves earlier MMX and XMM writes",
        &code,
        &image,
        &[
            dispatch(mmx_dirty),
            dispatch(xmm_dirty),
            Step {
                cpu: xmm_dirty,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            },
        ],
    );
}

fn restored_conversion_controls(frontend: Frontend) {
    let restore = instruction(FLAT32, true, 0x4010);
    let code = [
        &[0x0f, 0xae, 0x15, 0x10, 0x46, 0, 0][..], // LDMXCSR [4610]
        &[0x66, 0x0f, 0xef, 0xed],                 // PXOR xmm5, xmm5
        &restore,
        &[0x0f, 0x2d, 0xf5], // CVTPS2PI mm6, xmm5
    ]
    .concat();
    let mut sequences = ImageSequences::new(frontend, FLAT32);
    for unmasked_precision in [false, true] {
        let mut image = image(&code, FLAT32);
        image.data(0x8610, &0x7f80_u32.to_le_bytes()); // Old rounding: truncate.
        let mut source = image.cpu;
        source.x87.status = status(0x2800);
        source.x87.tag_word = 0xeeee;
        // Restored rounding is toward positive infinity in both cases.
        source.simd.mxcsr = if unmasked_precision { 0x4f80 } else { 0x5f80 };
        source.simd.xmm[5][..4].copy_from_slice(&1.5_f32.to_bits().to_le_bytes());
        source.simd.xmm[5][4..8].copy_from_slice(&(-1.5_f32).to_bits().to_le_bytes());
        for index in 0..8 {
            write_register_bits(
                &mut source,
                index,
                (0x8123_4567_89ab_cd00 + index as u64, 0xffff),
            );
        }
        let bytes = saved_bytes(
            &source,
            [
                0x2800_037f,
                0x0654_0055,
                0x1234_5678,
                0x17,
                0x89ab_cdef,
                0x27,
                source.simd.mxcsr,
                0xffff,
            ],
            [5, 6, 7, 0, 1, 2, 3, 4],
        );
        image.data(0x8010, &bytes);
        let mut old_controls = retire(image.cpu, 7);
        old_controls.simd.mxcsr = 0x7f80;
        let mut old_xmm = retire(old_controls, 4);
        old_xmm.simd.xmm[5] = [0; 16];
        let mut loaded = retire(old_xmm, restore.len());
        loaded.x87 = source.x87;
        loaded.simd = source.simd;
        let mut converted = if unmasked_precision {
            loaded
        } else {
            retire(loaded, 3)
        };
        converted.simd.mxcsr |= 0x20;
        let exit = if unmasked_precision {
            Exit::SimdFloatingPoint
        } else {
            converted.x87.status.top = 0;
            converted.x87.tag_word = 0;
            // Round 1.5 to 2 and -1.5 to -1, independently of old controls.
            write_register_bits(&mut converted, 6, (0xffff_ffff_0000_0002, 0xffff));
            Exit::Dispatch(converted.eip)
        };
        sequences.check(
            "MMX conversion uses restored XMM data and MXCSR rounding/masks before transition",
            &code,
            &image,
            &[
                dispatch(old_controls),
                dispatch(old_xmm),
                dispatch(loaded),
                Step {
                    cpu: converted,
                    ram: &[],
                    exit,
                },
            ],
        );
    }
}

test_frontends!(fxrstor_mmx_xmm_transfers, restored_transfers);
test_frontends!(fxrstor_mmx_xmm_fault, failed_restore);
test_frontends!(fxrstor_mxcsr_mmx_conversion, restored_conversion_controls);
