use super::*;

fn transfers(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (destination, source) in [(0, 7), (7, 0), (2, 2)] {
        for prefix in [0xf3, 0xf2] {
            let code = [prefix, 0x0f, 0xd6, 0xc0 | destination << 3 | source];
            let mut image = image(&code);
            let bits = 0xfedcba98_76543210_u64;
            write_register_bits(&mut image.cpu, source as usize, (bits, 0x1234));
            image.cpu.simd.xmm[source as usize] =
                0x11223344_55667788_88776655_44332211_u128.to_le_bytes();
            let mut cpu = complete(image.cpu, code.len() as u32);
            if prefix == 0xf3 {
                cpu.simd.xmm[destination as usize] = u128::from(bits).to_le_bytes();
            } else {
                write_register_bits(
                    &mut cpu,
                    destination as usize,
                    (0x88776655_44332211, 0xffff),
                );
            }
            checks.check(
                "MMX/XMM transfers use physical slots and preserve the source register",
                &code,
                &image,
                &[dispatch(cpu)],
            );
            image.cpu.x87.status.error_summary = 1;
            checks.check(
                "pending x87 exception precedes transfer and MMX environment effects",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                }],
            );
        }
    }

    let code = [0xf3, 0x0f, 0xd6, 0xf2, 0xf2, 0x0f, 0xd6, 0xfe, 0x0f, 0x77];
    let mut image = image(&code); // MOVQ2DQ xmm6,mm2; MOVDQ2Q mm7,xmm6; EMMS
    let bits = 0x01234567_89abcdef_u64;
    write_register_bits(&mut image.cpu, 2, (bits, 0x1234));
    let mut to_xmm = complete(image.cpu, 4);
    to_xmm.simd.xmm[6] = u128::from(bits).to_le_bytes();
    let mut to_mmx = complete(to_xmm, 4);
    write_register_bits(&mut to_mmx, 7, (bits, 0xffff));
    let mut emptied = complete(to_mmx, 2);
    emptied.x87.tag_word = 0xffff;
    checks.check(
        "transfer chain and EMMS",
        &code,
        &image,
        &[dispatch(to_xmm), dispatch(to_mmx), dispatch(emptied)],
    );
}

test_frontends!(mmx_xmm_transfers, transfers);

#[test]
fn mmx_xmm_transfers_reject_memory_before_fetching_address_bytes() {
    for prefix in [0xf2, 0xf3] {
        for modrm in [0x00, 0x04, 0x40, 0x80] {
            crate::support::encoding::check_unsupported_at_page_end(
                &[prefix, 0x0f, 0xd6, modrm],
                prefix,
            );
        }
    }
}
