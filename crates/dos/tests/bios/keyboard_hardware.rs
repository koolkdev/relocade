use crate::common;
use relocade_dos::Dos;
use relocade_machine::IoBus;
use relocade_x86::{ExecutionExit, HostExit};

fn inject_scan(io: &IoBus, byte: u8) {
    // 8042 D2h writes its output register, exercising the normal IRQ1 path.
    io.write_port(relocade_at_keyboard::STATUS_COMMAND_PORT, 1, 0xd2);
    io.write_port(relocade_at_keyboard::DATA_PORT, 1, u32::from(byte));
}

#[test]
fn cli_allows_guest_port_polling_without_bios_consuming_the_scan_byte() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(
            &[
                0xfa, // CLI.
                0xe4, 0x64, 0xa2, 0, 3, // IN AL,64h; save status.
                0xe4, 0x60, 0xa2, 1, 3, // IN AL,60h; save scan byte.
                0xe4, 0x64, 0xa2, 2, 3, // IN AL,64h; save drained status.
                0xb8, 0, 0x4c, 0xcd, 0x21, // Exit.
            ],
            Default::default(),
        )
        .unwrap();
        dos.cpu().cpu_state().set_eflags(0x202);
        common::steps(dos.cpu(), 1); // Execute CLI before input arrives.
        assert!(!dos.cpu().cpu_state().interrupts_enabled());
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        inject_scan(dos.cpu().context().io, 0x1e);
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_ne!(memory.view().read_u8(base + 0x300).unwrap() & 1, 0);
        assert_eq!(memory.view().read_u8(base + 0x301).unwrap(), 0x1e);
        assert_eq!(memory.view().read_u8(base + 0x302).unwrap() & 1, 0);
        assert_eq!(dos.keyboard().peek_key(), None);
    });
}

async fn guest_returns(dos: &mut Dos) {
    assert_eq!(
        common::run_until(dos.cpu(), 0x100).await,
        ExecutionExit::Interrupted
    );
}

#[test]
fn replacing_irq1_owns_scan_consumption_and_guest_eoi_releases_the_next_irq() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(&[0xeb, 0xfe], Default::default()).unwrap();
        dos.cpu().cpu_state().set_eflags(0x202);
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let old_mask = io.read_port(relocade_pic::MASK_PORT, 1);
        io.write_port(
            relocade_pic::MASK_PORT,
            1,
            old_mask | (1 << relocade_pic::KEYBOARD_IRQ),
        );
        memory
            .view()
            .write_u32(9 * 4, (u32::from(segment) << 16) | 0x180)
            .unwrap();
        memory
            .view()
            .write(
                base + 0x180,
                &[
                    0x50, 0x53, 0x1e, 0x0e, 0x1f, // Save AX/BX/DS; DS=CS.
                    0xe4, 0x60, // Consume the scan byte.
                    0x8b, 0x1e, 0, 3, // BX = scan count.
                    0x88, 0x87, 2, 3, // Save AL in scan log.
                    0xff, 0x06, 0, 3, // Increment count.
                    0x1f, 0x5b, 0x58, 0xcf, // Restore; IRET without EOI.
                ],
            )
            .unwrap();
        memory
            .view()
            .write(
                base + 0x110,
                &[
                    0xb0, 0x20, 0xe6, 0x20, // Non-specific EOI to the master PIC.
                    0xe9, 0xe9, 0xff, // JMP 0100h.
                ],
            )
            .unwrap();
        inject_scan(&io, 0x1e);
        guest_returns(&mut dos).await;
        assert_eq!(
            memory.view().read_u16(base + 0x300).unwrap(),
            0,
            "masked IRQ1 stays pending"
        );
        io.write_port(relocade_pic::MASK_PORT, 1, old_mask);
        guest_returns(&mut dos).await;
        assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 1);
        inject_scan(&io, 0x9e);
        guest_returns(&mut dos).await;
        assert_eq!(
            memory.view().read_u16(base + 0x300).unwrap(),
            1,
            "in-service IRQ1 blocks delivery"
        );
        dos.cpu().cpu_state().set_eip(0x110);
        guest_returns(&mut dos).await;
        assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 2);
        assert_eq!(memory.view().read_u16(base + 0x302).unwrap(), 0x9e1e);
        assert_eq!(
            dos.keyboard().peek_key(),
            None,
            "replacement ISR bypasses BIOS translation"
        );
    });
}

#[test]
fn a_guest_irq1_hook_chains_to_bios_and_wakes_both_input_services() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(
            &[0xcd, 0x16, 0xeb, 0xfe, 0xcd, 0x21, 0xeb, 0xfe],
            Default::default(),
        )
        .unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let original = memory.view().read_u32(9 * 4).unwrap();
        let mut handler = vec![0x2e, 0xff, 0x06, 0, 3, 0xea]; // INC CS:[0300h]; JMP FAR BIOS.
        handler.extend(original.to_le_bytes());
        memory.view().write(base + 0x180, &handler).unwrap();
        memory
            .view()
            .write_u32(9 * 4, (u32::from(segment) << 16) | 0x180)
            .unwrap();
        for (ip, ax, scan, expected) in [(0x100, 0, 0x1e, 0x1e61), (0x104, 0x0700, 0x30, 0x0762)] {
            dos.cpu().cpu_state().set_eip(ip);
            dos.cpu().cpu_state().set_ax(ax);
            dos.cpu().cpu_state().set_eflags(2);
            common::assert_pending(common::run_until(dos.cpu(), ip + 2));
            inject_scan(&io, scan);
            assert_eq!(
                common::run_until(dos.cpu(), ip + 2).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(dos.cpu().cpu_state().ax(), expected);
            assert!(
                !dos.cpu().cpu_state().interrupts_enabled(),
                "IRET restores caller IF"
            );
        }
        assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 2);
        assert_eq!(dos.keyboard().peek_key(), None);
        // INT16 reads a word inserted by guest software into the same ring.
        let tail = memory.view().read_u16(0x41c).unwrap();
        memory
            .view()
            .write_u16(0x400 + u32::from(tail), 0x2c5a)
            .unwrap();
        memory.view().write_u16(0x41c, tail + 2).unwrap();
        dos.cpu().cpu_state().set_eip(0x100);
        dos.cpu().cpu_state().set_ax(0);
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().cpu_state().ax(), 0x2c5a);
        assert_eq!(dos.keyboard().peek_key(), None);
    });
}
