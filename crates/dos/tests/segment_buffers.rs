mod common;

use common::{path_service, service, success, Backend, SERVICE_PROGRAM};
use relocade_dos::{Dos, DosServices, FileAccess};
use relocade_x86::{ExecutionExit, Segment, SegmentRegister};
use std::io::Write;

const BASE: u32 = 0x30000;

fn use_data_segment(dos: &mut Dos<Backend<DosServices>>) {
    // Keep buffers away from the program and interrupt stack.
    dos.runtime()
        .machine()
        .cpu()
        .set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x3000));
}

fn write_memory(dos: &mut Dos<Backend<DosServices>>, address: u32, bytes: &[u8]) {
    dos.runtime()
        .machine()
        .memory()
        .write(address, bytes)
        .unwrap();
}

fn read_memory<const N: usize>(dos: &mut Dos<Backend<DosServices>>, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    dos.runtime()
        .machine()
        .memory()
        .read(address, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn file_transfers_stop_at_the_segment_end_and_advance_only_by_the_returned_count() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("IN.BIN", b"ABCD".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let input = success(&path_service(&mut dos, b"IN.BIN", 0x3d00, 0));
        let output = success(&path_service(&mut dos, b"OUT.BIN", 0x3c00, 0));
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xfffe, &[0xcc; 4]);
        write_memory(&mut dos, BASE, &[0xdd; 4]);

        assert_eq!(success(&service(&mut dos, 0x3f00, input, 0, 0xffff)), 0);
        assert_eq!(success(&service(&mut dos, 0x3f00, input, 4, 0xfffe)), 2);
        assert_eq!(
            read_memory::<4>(&mut dos, BASE + 0xfffe),
            [b'A', b'B', 0xcc, 0xcc]
        );
        assert_eq!(read_memory::<4>(&mut dos, BASE), [0xdd; 4]);
        assert_eq!(success(&service(&mut dos, 0x3f00, input, 4, 0)), 2);
        assert_eq!(read_memory::<4>(&mut dos, BASE), [b'C', b'D', 0xdd, 0xdd]);

        write_memory(&mut dos, BASE + 0xffff, b"WXY");
        assert_eq!(success(&service(&mut dos, 0x4000, output, 3, 0xffff)), 1);
        assert_eq!(success(&service(&mut dos, 0x4000, output, 2, 0)), 2);
        assert_eq!(dos.read_file("OUT.BIN").unwrap(), b"WCD");
        // CX=0 retains its file-length meaning even at the last segment byte.
        success(&service(&mut dos, 0x4200, output, 0, 1));
        assert_eq!(success(&service(&mut dos, 0x4000, output, 0, 0xffff)), 0);
        assert_eq!(dos.read_file("OUT.BIN").unwrap(), b"W");
    });
}

#[test]
fn deferred_console_read_keeps_the_bytes_that_do_not_fit_in_the_segment() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xffff, &[0xcc; 3]);
        {
            let mut machine = dos.runtime().machine();
            let mut cpu = machine.cpu();
            cpu.set_ax(0x3f00);
            cpu.set_bx(0);
            cpu.set_cx(3);
            cpu.set_dx(0xffff);
        }
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        dos.stdin().write_all(b"abc\r").unwrap();
        assert_eq!(
            dos.runtime().run_async(10).await.unwrap(),
            ExecutionExit::BudgetExhausted
        );
        assert_eq!(dos.runtime().machine().cpu().ax(), 1);
        assert_eq!(
            read_memory::<3>(&mut dos, BASE + 0xffff),
            [b'a', 0xcc, 0xcc]
        );
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 4, 0)), 4);
        assert_eq!(&read_memory::<4>(&mut dos, BASE), b"bc\r\n");
        assert_eq!(common::output(dos.stdout()), b"abc\r\n");

        write_memory(&mut dos, BASE + 0xffff, b"WXY");
        assert_eq!(success(&service(&mut dos, 0x4000, 1, 3, 0xffff)), 1);
        assert_eq!(common::output(dos.stdout()), b"W");
    });
}

#[test]
fn current_directory_wraps_its_destination_offset() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"DIR", 0x3900, 0));
        success(&path_service(&mut dos, b"DIR", 0x3b00, 0));
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xffff, &[0xcc; 4]);
        write_memory(&mut dos, BASE, &[0xdd; 4]);
        dos.runtime().machine().cpu().set_si(0xffff);

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
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        use_data_segment(&mut dos);
        write_memory(&mut dos, BASE + 0xffff, b"Abad$");
        write_memory(&mut dos, BASE, b"B$");

        service(&mut dos, 0x0900, 0, 0, 0xffff);
        assert_eq!(common::output(dos.stdout()), b"AB");
        assert_eq!(dos.runtime().machine().cpu().al(), b'$');
    });
}

#[test]
fn buffered_input_wraps_its_payload_after_the_header() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
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
