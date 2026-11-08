mod common;

use common::{path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};
use relocade_x86::{ExecutionExit, Segment, SegmentRegister};
use std::io::Write;

const BASE: u32 = 0x30000;

fn use_data_segment(dos: &mut Dos) {
    // Keep buffers away from the program and interrupt stack.
    dos.cpu()
        .context()
        .cpu
        .set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x3000));
}

fn write_memory(dos: &mut Dos, address: u32, bytes: &[u8]) {
    dos.cpu().context().memory.write(address, bytes).unwrap();
}

fn read_memory<const N: usize>(dos: &mut Dos, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    dos.cpu()
        .context()
        .memory
        .read(address, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn file_transfers_cross_the_segment_end_and_advance_by_the_returned_count() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("IN.BIN", b"ABCD".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let input = success(&path_service(&mut dos, b"IN.BIN", 0x3d00, 0));
        let output = success(&path_service(&mut dos, b"OUT.BIN", 0x3c00, 0));
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xfffe, &[0xcc; 4]);
        write_memory(&mut dos, BASE, &[0xdd; 4]);

        assert_eq!(success(&service(&mut dos, 0x3f00, input, 0, 0xffff)), 0);
        assert_eq!(success(&service(&mut dos, 0x3f00, input, 4, 0xfffe)), 4);
        assert_eq!(&read_memory::<4>(&mut dos, BASE + 0xfffe), b"ABCD");
        assert_eq!(read_memory::<4>(&mut dos, BASE), [0xdd; 4]);
        assert_eq!(success(&service(&mut dos, 0x3f00, input, 4, 0)), 0);
        assert_eq!(read_memory::<4>(&mut dos, BASE), [0xdd; 4]);

        write_memory(&mut dos, BASE + 0xffff, b"WXY");
        write_memory(&mut dos, BASE, b"Z!");
        assert_eq!(success(&service(&mut dos, 0x4000, output, 3, 0xffff)), 3);
        assert_eq!(success(&service(&mut dos, 0x4000, output, 2, 0)), 2);
        assert_eq!(dos.read_file("OUT.BIN").unwrap(), b"WXYZ!");
        // CX=0 retains its file-length meaning even at the last segment byte.
        success(&service(&mut dos, 0x4200, output, 0, 1));
        assert_eq!(success(&service(&mut dos, 0x4000, output, 0, 0xffff)), 0);
        assert_eq!(dos.read_file("OUT.BIN").unwrap(), b"W");
    });
}

#[test]
fn pending_console_handle_transfers_cross_the_segment_end() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xffff, &[0xcc; 3]);
        write_memory(&mut dos, BASE, &[0xdd; 4]);
        {
            let mut machine = dos.cpu().context();
            let cpu = &mut machine.cpu;
            cpu.set_ax(0x3f00);
            cpu.set_bx(0);
            cpu.set_cx(3);
            cpu.set_dx(0xffff);
        }
        crate::common::assert_pending(dos.cpu().run());
        dos.stdin().write_all(b"abc\r").unwrap();
        assert_eq!(
            crate::common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().context().cpu.ax(), 3);
        assert_eq!(&read_memory::<3>(&mut dos, BASE + 0xffff), b"abc");
        assert_eq!(read_memory::<4>(&mut dos, BASE), [0xdd; 4]);
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 4, 0)), 2);
        assert_eq!(&read_memory::<4>(&mut dos, BASE), b"\r\n\xdd\xdd");
        assert_eq!(common::output(dos.stdout()), b"abc\r\n");

        write_memory(&mut dos, BASE + 0xffff, b"WXY");
        assert_eq!(success(&service(&mut dos, 0x4000, 1, 3, 0xffff)), 3);
        assert_eq!(common::output(dos.stdout()), b"WXY");
    });
}

#[test]
fn large_file_read_crosses_64k_and_stops_at_eof() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let contents: Vec<u8> = (0..89_116).map(|index| (index % 251) as u8).collect();
        dos.mount_file("BUMSPJEU.BIN", contents.clone(), FileAccess::ReadOnly)
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let handle = success(&path_service(&mut dos, b"BUMSPJEU.BIN", 0x3d00, 0));
        dos.cpu()
            .context()
            .cpu
            .set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x344c));
        let base = 0x344c0;
        write_memory(&mut dos, base, &vec![0xcc; 90_000]);

        // Bumpy loads its sprite bank in these two reads into adjacent buffers.
        assert_eq!(
            success(&service(&mut dos, 0x3f00, handle, 64_000, 0)),
            64_000
        );
        assert_eq!(
            success(&service(&mut dos, 0x3f00, handle, 26_000, 0xfa00)),
            25_116
        );
        let mut bytes = vec![0; 90_000];
        dos.cpu().context().memory.read(base, &mut bytes).unwrap();
        assert_eq!(&bytes[..89_116], contents.as_slice());
        assert_eq!(&bytes[89_116..], &[0xcc; 884]);
        let cursor = service(&mut dos, 0x4201, handle, 0, 0);
        assert_eq!(
            ((cursor.registers.edx & 0xffff) << 16) | u32::from(success(&cursor)),
            89_116
        );
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 1, 0)), 0);
    });
}

#[test]
fn current_directory_wraps_its_destination_offset() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"DIR", 0x3900, 0));
        success(&path_service(&mut dos, b"DIR", 0x3b00, 0));
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xffff, &[0xcc; 4]);
        write_memory(&mut dos, BASE, &[0xdd; 4]);
        dos.cpu().context().cpu.set_si(0xffff);

        assert_eq!(success(&service(&mut dos, 0x4700, 0, 0, 0)), 0x0100);
        assert_eq!(read_memory::<4>(&mut dos, BASE), [b'I', b'R', 0, 0xdd]);
        assert_eq!(
            read_memory::<4>(&mut dos, BASE + 0xffff),
            [b'D', 0xcc, 0xcc, 0xcc]
        );
    });
}

#[test]
fn dollar_terminated_output_wraps_its_source_offset() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xffff, b"Abad$");
        write_memory(&mut dos, BASE, b"B$");

        service(&mut dos, 0x0900, 0, 0, 0xffff);
        assert_eq!(common::output(dos.stdout()), b"AB");
        assert_eq!(dos.cpu().context().cpu.al(), b'$');
    });
}

#[test]
fn buffered_input_wraps_its_payload_after_the_header() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xfffd, &[4, 0, 0xcc, 0xcc, 0xcc]);
        write_memory(&mut dos, BASE, &[0xdd; 3]);
        dos.stdin().write_all(b"xy\r").unwrap();

        service(&mut dos, 0x0a00, 0, 0, 0xfffd);
        assert_eq!(
            read_memory::<5>(&mut dos, BASE + 0xfffd),
            [4, 2, b'x', 0xcc, 0xcc]
        );
        assert_eq!(read_memory::<3>(&mut dos, BASE), [b'y', b'\r', 0xdd]);
        assert_eq!(common::output(dos.stdout()), b"xy\r");
    });
}
