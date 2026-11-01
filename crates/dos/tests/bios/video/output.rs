use super::{video, PROGRAM};
use crate::common::{self, vga_support::software};

#[test]
fn screen_output_changes_only_the_sequencer_bit_and_acknowledges_invalid_selections() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for mode in [3, 7, 0x13] {
            video(&mut dos, mode, 0, 0, 0);
            // Keep nondefault clocking bits to detect an accidental register reset.
            dos.cpu().context().io.write_port(0x3c4, 2, 0x1d01);
            let before = dos.vga().borrow().snapshot();
            for (selection, clocking) in [(1, 0x3d), (0, 0x1d)] {
                dos.cpu().context().io.write_port(0x3c4, 1, 4);
                let result = video(&mut dos, 0x1200 | selection, 0xa536, 0x5678, 0x9abc);
                assert_eq!(result.registers.eax as u16, 0x1212);
                assert_eq!(result.registers.ebx as u16, 0xa536);
                assert_eq!(result.registers.ecx as u16, 0x5678);
                assert_eq!(result.registers.edx as u16, 0x9abc);
                let after = dos.vga().borrow().snapshot();
                let mut expected = before.registers.clone();
                expected.sequencer.values[1] = clocking;
                assert_eq!(after.registers, expected); // Includes the final sequencer index.
                for plane in 0..4 {
                    assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
                }
            }
            for selection in [2, 255] {
                dos.cpu().context().io.write_port(0x3c4, 1, 4);
                let before = dos.vga().borrow().snapshot();
                let result = video(&mut dos, 0x1200 | selection, 0x36, 0, 0);
                assert_eq!(result.registers.eax as u16, 0x1212);
                assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
            }
        }
    });
}

#[test]
fn blanked_output_keeps_capturing_memory_writes_and_reveals_them_when_enabled() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        // Keep cursor blink out of the pixel checks.
        video(&mut dos, 0x0100, 0, 0x2007, 0);
        // Sequencer blanking asserts the DAC's blank input, regardless of color zero.
        video(&mut dos, 0x1010, 0, 0, 0x3f00);
        let initial = common::next_capture(&dos).await;
        video(&mut dos, 0x1201, 0x36, 0, 0);
        let blank = common::next_capture(&dos).await;
        assert!(blank.sequence > initial.sequence);
        assert_eq!(blank.display.registers.sequencer.values[1], 0x21);
        for plane in 0..4 {
            assert!(std::ptr::eq(
                blank.display.plane(plane),
                initial.display.plane(plane)
            ));
        }
        // The CPU can still read and write the text aperture while output is off.
        dos.cpu()
            .context()
            .memory
            .write_u16(0xb8000, 0x4f41)
            .unwrap();
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb8000).unwrap(),
            0x4f41
        );
        let written = common::next_capture(&dos).await;
        assert!(written.sequence > blank.sequence);
        assert!(written.time > blank.time);
        assert_eq!(written.display.plane(0)[0], b'A');
        assert_eq!(written.display.plane(1)[0], 0x4f);
        assert!(software::render(&written.display)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel == &[0, 0, 0, 255]));

        video(&mut dos, 0x1200, 0x36, 0, 0);
        let visible = common::next_capture(&dos).await;
        assert_eq!(visible.display.registers.sequencer.values[1], 1);
        for plane in 0..4 {
            assert!(std::ptr::eq(
                visible.display.plane(plane),
                written.display.plane(plane)
            ));
        }
        // A's first glyph row starts with a background dot and a foreground dot.
        assert_eq!(
            &software::render(&visible.display)[..8],
            &[170, 0, 0, 255, 255, 255, 255, 255]
        );
        assert_eq!(initial.display.plane(0)[0], b' '); // The retained capture is immutable.

        video(&mut dos, 0x1201, 0x36, 0, 0);
        video(&mut dos, 0x83, 0, 0, 0); // A mode set restores output while preserving cells.
        let reset = dos.vga().borrow().snapshot();
        assert_eq!(reset.registers.sequencer.values[1], 1);
        assert_eq!(reset.plane(0)[0], b'A');
    });
}
