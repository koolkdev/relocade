use crate::common;
use relocade_x86::ExecutionExit;

#[test]
fn equipment_and_memory_queries_read_the_live_bios_data_area() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let memory = dos.cpu().memory();
        assert_eq!(
            memory.view().read_u16(0x410).unwrap(),
            6,
            "x87, VGA and pointing device; no diskette, serial or parallel ports"
        );
        assert_eq!(memory.view().read_u16(0x413).unwrap(), 640);
        assert_eq!(relocade_pc::CONVENTIONAL_MEMORY_SIZE, 640 * 1024);
        // INT 11h; MOV BX,AX; INT 12h; loop.
        dos.launch_com(
            &[0xcd, 0x11, 0x89, 0xc3, 0xcd, 0x12, 0xeb, 0xfe],
            Default::default(),
        )
        .unwrap();
        for (equipment, kib) in [(2, 640), (0xa5b2, 639)] {
            memory.view().write_u16(0x410, equipment).unwrap();
            memory.view().write_u16(0x413, kib).unwrap();
            let state = dos.cpu().cpu_state();
            state.set_eip(0x100);
            state.set_ax(0xffff);
            state.set_cx(0x1234);
            state.set_dx(0x5678);
            state.set_eflags(0x643);
            assert_eq!(
                common::run_until(dos.cpu(), 0x106).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(state.ax(), kib);
            assert_eq!(state.bx(), equipment);
            assert_eq!(state.cx(), 0x1234);
            assert_eq!(state.dx(), 0x5678);
            assert_eq!(state.sp(), 0xfffe);
            assert_eq!(state.eflags(), 0x643);
        }
        // Video mode changes preserve the rest of the guest equipment word.
        memory
            .view()
            .write(0x10100, &[0xcd, 0x10, 0xeb, 0xfe])
            .unwrap();
        let state = dos.cpu().cpu_state();
        for mode in [7, 3] {
            state.set_eip(0x100);
            state.set_ax(mode);
            assert_eq!(
                common::run_until(dos.cpu(), 0x102).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(memory.view().read_u16(0x410).unwrap(), 0xa5b2);
            assert_eq!(memory.view().read_u16(0x413).unwrap(), 639);
        }
    });
}
