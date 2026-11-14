use super::{common, service};
use common::{error, success, SERVICE_PROGRAM};
use relocade_dos::Dos;

async fn arena() -> Dos {
    let mut dos = common::dos().await;
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    success(&service(&mut dos, 0x4a, 0x1000, 0x1000));
    dos
}

async fn fragmented_arena() -> Dos {
    let mut dos = arena().await;
    // Three holes separated by live blocks. The first is larger; the two
    // smaller holes distinguish best-fit's low-address tie from last fit.
    for (paragraphs, segment) in [
        (0x30, 0x2001),
        (1, 0x2032),
        (0x20, 0x2034),
        (1, 0x2055),
        (0x20, 0x2057),
        (1, 0x2078),
        (0x7f84, 0x207a),
    ] {
        assert_eq!(success(&service(&mut dos, 0x48, paragraphs, 0)), segment);
    }
    for segment in [0x2001, 0x2034, 0x2057] {
        success(&service(&mut dos, 0x49, 0, segment));
    }
    dos
}

fn set_strategy(dos: &mut Dos, strategy: u16) {
    success(&common::service(dos, 0x5801, strategy, 0, 0));
}

fn header(dos: &mut Dos, segment: u16) -> [u8; 5] {
    let mut bytes = [0; 5];
    dos.cpu()
        .memory()
        .view()
        .read(u32::from(segment - 1) * 16, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn strategy_services_validate_bx_and_preserve_unrelated_registers_and_flags() {
    pollster::block_on(async {
        let mut dos = arena().await;
        for (ax, bx, result_ax, carry) in [
            (0x5800, 0x1234, 0, false), // Default: first fit.
            (0x5801, 1, 0x5801, false),
            (0x5800, 0x1234, 1, false),
            (0x5801, 2, 0x5801, false),
            (0x5800, 0x1234, 2, false),
            (0x5801, 3, 1, true),
            (0x5801, 0x3f, 1, true),
            (0x5801, 0x43, 1, true),
            (0x5801, 0x83, 1, true),
            (0x5801, 0xc0, 1, true),
            (0x5801, 0x0100, 1, true), // Validate BH as well as BL.
            (0x5801, 0xffff, 1, true),
            (0x5802, 0x1234, 0x5800, false), // Only AL receives the link state.
            (0x5803, 0, 1, true), // No UMB provider, even for unlink.
            (0x5803, 1, 1, true),
            (0x5803, 2, 1, true),
            (0x5803, 0x0100, 1, true),
            (0x5803, 0x0101, 1, true),
            (0x5802, 0, 0x5800, false),
            (0x5804, 0, 1, true),
            (0x58ff, 0, 1, true),
            (0x5800, 0x1234, 2, false), // Rejections preserve the policy.
            (0x5801, 0, 0x5801, false),
            (0x5800, 0x1234, 0, false),
        ] {
            let cpu = dos.cpu().cpu_state();
            cpu.set_eax(0xabcd_0000 | u32::from(ax));
            cpu.set_ebx(0x1234_0000 | u32::from(bx));
            cpu.set_ecx(0x2345_6789);
            cpu.set_edx(0x3456_789a);
            cpu.set_esi(0x4567_89ab);
            cpu.set_edi(0x5678_9abc);
            cpu.set_eflags(if carry { 0xa56 } else { 0xa57 });
            let before = cpu.snapshot();
            let after = common::service(&mut dos, ax, bx, 0x6789, 0x789a);
            let mut registers = before.registers;
            registers.eax = 0xabcd_0000 | result_ax;
            assert_eq!(after.registers, registers);
            assert_eq!(after.segments, before.segments);
            assert_eq!(after.flags.eflags(), 0xa56 | u32::from(carry));
        }
    });
}

#[test]
fn fit_strategies_choose_and_split_fragmented_free_blocks() {
    pollster::block_on(async {
        let mut dos = fragmented_arena().await;
        for (strategy, segment, zero_segment, exact_segment) in [
            (0, 0x2001, 0x2001, 0x2001),
            (1, 0x2034, 0x2034, 0x2034),
            (2, 0x2067, 0x2077, 0x2057),
        ] {
            set_strategy(&mut dos, strategy);
            assert_eq!(success(&service(&mut dos, 0x48, 0x10, 0)), segment);
            assert_eq!(header(&mut dos, segment), [b'M', 0, 0x10, 0x10, 0]);
            success(&service(&mut dos, 0x49, 0, segment));

            let failed = service(&mut dos, 0x48, 0x31, 0);
            error(&failed, 8);
            assert_eq!(failed.registers.ebx as u16, 0x30);

            assert_eq!(success(&service(&mut dos, 0x48, 0, 0)), zero_segment);
            assert_eq!(header(&mut dos, zero_segment), [b'M', 0, 0x10, 0, 0]);
            success(&service(&mut dos, 0x49, 0, zero_segment));

            assert_eq!(success(&service(&mut dos, 0x48, 0x20, 0)), exact_segment);
            success(&service(&mut dos, 0x49, 0, exact_segment));
        }
    });
}

#[test]
fn upper_memory_strategies_fall_back_while_umbs_are_unlinked() {
    pollster::block_on(async {
        let mut dos = fragmented_arena().await;
        for (strategy, segment) in [
            (0x40, 0x2001),
            (0x41, 0x2034),
            (0x42, 0x2067),
            (0x80, 0x2001),
            (0x81, 0x2034),
            (0x82, 0x2067),
        ] {
            set_strategy(&mut dos, strategy);
            assert_eq!(
                success(&common::service(&mut dos, 0x5800, 0, 0, 0)),
                strategy
            );
            assert_eq!(success(&common::service(&mut dos, 0x5802, 0, 0, 0)), 0x5800);
            assert_eq!(success(&service(&mut dos, 0x48, 0x10, 0)), segment);
            success(&service(&mut dos, 0x49, 0, segment));
        }
    });
}

#[test]
fn last_fit_preserves_the_terminal_mcb_through_resize_and_free() {
    pollster::block_on(async {
        let mut dos = arena().await;
        // Release the environment so the free block reaches the end of RAM.
        success(&service(&mut dos, 0x49, 0, 0x9fff));
        set_strategy(&mut dos, 2);
        assert_eq!(success(&service(&mut dos, 0x48, 0x10, 0)), 0x9ff0);
        assert_eq!(header(&mut dos, 0x9ff0), [b'Z', 0, 0x10, 0x10, 0]);
        assert_eq!(header(&mut dos, 0x2001), [b'M', 0, 0, 0xee, 0x7f]);
        success(&service(&mut dos, 0x4a, 8, 0x9ff0));
        assert_eq!(header(&mut dos, 0x9ff0), [b'M', 0, 0x10, 8, 0]);
        assert_eq!(header(&mut dos, 0x9ff9), [b'Z', 0, 0, 7, 0]);
        success(&service(&mut dos, 0x49, 0, 0x9ff0));

        // Exact fit, an MCB-only low remainder, and an MCB-only allocation.
        for (paragraphs, segment) in [(0x7fff, 0x2001), (0x7ffe, 0x2002), (0, 0xa000)] {
            assert_eq!(success(&service(&mut dos, 0x48, paragraphs, 0)), segment);
            let [lo, hi] = paragraphs.to_le_bytes();
            assert_eq!(header(&mut dos, segment), [b'Z', 0, 0x10, lo, hi]);
            success(&service(&mut dos, 0x49, 0, segment));
            assert_eq!(header(&mut dos, 0x2001), [b'Z', 0, 0, 0xff, 0x7f]);
        }
    });
}

#[test]
fn allocation_strategy_survives_process_termination() {
    pollster::block_on(async {
        let mut dos = arena().await;
        set_strategy(&mut dos, 2);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(success(&common::service(&mut dos, 0x5800, 0, 0, 0)), 2);
        success(&service(&mut dos, 0x4a, 0x1000, 0x1000));
        assert_eq!(success(&service(&mut dos, 0x48, 0x10, 0)), 0x9fee);
    });
}
