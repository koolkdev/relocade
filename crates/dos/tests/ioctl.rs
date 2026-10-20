mod common;

use common::{error, path_service, service, success, Backend, SERVICE_PROGRAM};
use relocade_dos::{Dos, DosState, FileAccess};
use std::io::Write;

fn information(dos: &mut Dos<Backend<DosState>>, handle: u16) -> u16 {
    let cpu = service(dos, 0x4400, handle, 0, 0xffff);
    assert_eq!(success(&cpu), cpu.registers.edx as u16);
    cpu.registers.edx as u16
}

async fn check_device_information() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA.BIN", b"ab".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos.runtime().machine().cpu().set_eflags(0x0a57);
    // CON includes both the driver and handle device bits, input/output, and no input EOF.
    for handle in 0..=2 {
        assert_eq!(information(&mut dos, handle), 0x80c3);
    }
    assert_eq!(dos.runtime().machine().cpu().eflags(), 0x0a56);
    for handle in [3, 4] {
        assert_eq!(information(&mut dos, handle), 0x80c0); // AUX and PRN.
    }
    error(&service(&mut dos, 0x4400, 5, 0, 0), 6);
    error(&service(&mut dos, 0x4400, u16::MAX, 0, 0), 6);
    error(&service(&mut dos, 0x4401, 1, 0, 0), 1);
    error(&service(&mut dos, 0x44ff, 1, 0, 0), 1);
    assert_eq!(dos.exit_status(), None);

    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0)), 5);
    assert_eq!(success(&service(&mut dos, 0x4500, 5, 0, 0)), 6);
    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d40, 0)), 7);
    for handle in [5, 6, 7] {
        assert_eq!(information(&mut dos, handle), 0x0042); // Unwritten file on C:.
    }
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 1, 0x200)), 1);
    assert_eq!(information(&mut dos, 5), 0x0042); // Reading leaves the written flag clear.
    error(&service(&mut dos, 0x4000, 7, 1, 0x200), 5);
    assert_eq!(information(&mut dos, 7), 0x0042); // Failed writes do too.
    assert_eq!(success(&service(&mut dos, 0x4000, 6, 1, 0x200)), 1);
    assert_eq!(information(&mut dos, 5), 0x0002);
    assert_eq!(information(&mut dos, 6), 0x0002); // Duplicates share the written state.
    assert_eq!(information(&mut dos, 7), 0x0042); // Separate opens do not.

    success(&service(&mut dos, 0x4600, 5, 1, 0));
    assert_eq!(information(&mut dos, 1), 0x0002); // Redirected stdout is a file.
    success(&service(&mut dos, 0x4600, 2, 1, 0));
    assert_eq!(information(&mut dos, 1), 0x80c3); // Restored stdout is a device.
    success(&service(&mut dos, 0x3e00, 1, 0, 0));
    error(&service(&mut dos, 0x4400, 1, 0, 0), 6);

    // Creation/truncation and a successful zero-byte write also mark the open file.
    assert_eq!(success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0)), 1);
    assert_eq!(information(&mut dos, 1), 0x0002);
    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0)), 8);
    assert_eq!(information(&mut dos, 8), 0x0042);
    assert_eq!(success(&service(&mut dos, 0x4000, 8, 0, 0)), 0);
    assert_eq!(information(&mut dos, 8), 0x0002);
    assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"");
}

#[test]
fn ioctl_reports_the_object_and_write_state_behind_each_handle() {
    pollster::block_on(check_device_information());
}

#[test]
fn console_status_queries_do_not_consume_input_or_change_buffered_reads() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for flags in [0x602, 0x643] {
            dos.runtime().machine().cpu().set_eflags(flags);
            let cpu = service(&mut dos, 0x0b7b, 0x1234, 0x5678, 0x9abc);
            assert_eq!(cpu.registers.eax as u8, 0);
            assert_eq!(cpu.flags.eflags(), flags);
            assert_eq!(cpu.registers.ebx as u16, 0x1234);
            assert_eq!(cpu.registers.ecx as u16, 0x5678);
            assert_eq!(cpu.registers.edx as u16, 0x9abc);
        }
        dos.stdin().write_all(&[0, 0x48]).unwrap();
        for _ in 0..2 {
            dos.runtime().machine().cpu().set_eflags(0x643);
            let cpu = service(&mut dos, 0x0b00, 0, 0, 0);
            assert_eq!(cpu.registers.eax as u8, 0xff);
            assert_eq!(cpu.flags.eflags(), 0x643);
            // All three console handles query the same device, including stdout.
            for handle in 0..=2 {
                let cpu = service(&mut dos, 0x4406, handle, 0, 0x9abc);
                assert_eq!(success(&cpu) as u8, 0xff);
                assert_eq!(cpu.flags.eflags(), 0x642);
                assert_eq!(cpu.registers.edx as u16, 0x9abc);
            }
        }
        for byte in [0, 0x48] {
            assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, byte);
        }
        assert!(common::output(dos.stdout()).is_empty());

        dos.stdin().write_all(b"abc\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        // The completed DOS line is separate from the device's now-empty queue.
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
        assert_eq!(success(&service(&mut dos, 0x4406, 0, 0, 0)) as u8, 0);
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 4);
        let mut remainder = [0; 4];
        dos.runtime()
            .machine()
            .memory()
            .read(0x10400, &mut remainder)
            .unwrap();
        assert_eq!(&remainder, b"bc\r\n");

        dos.stdin().write_all(b"\x1a\rQ").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x8083);
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
        assert_eq!(success(&service(&mut dos, 0x4406, 0, 0, 0)) as u8, 0xff);
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x8083);
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'Q');
    });
}

#[test]
fn stdin_status_falls_back_to_the_console_when_stdin_is_closed() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&service(&mut dos, 0x4500, 0, 0, 0));
        success(&service(&mut dos, 0x3e00, 0, 0, 0));

        for (input, status) in [(b"".as_slice(), 0x00), (b"\0".as_slice(), 0xff)] {
            dos.stdin().write_all(input).unwrap();
            for flags in [0x602, 0x643] {
                dos.runtime().machine().cpu().set_eflags(flags);
                let cpu = service(&mut dos, 0x0b7b, 0x1234, 0x5678, 0x9abc);
                assert_eq!(cpu.registers.eax as u16, 0x0b00 | status);
                assert_eq!(cpu.flags.eflags(), flags);
                assert_eq!(cpu.registers.ebx as u16, 0x1234);
                assert_eq!(cpu.registers.ecx as u16, 0x5678);
                assert_eq!(cpu.registers.edx as u16, 0x9abc);
            }
            // Explicit handle queries still reject the closed handle.
            for function in [0x4406, 0x4407] {
                error(&service(&mut dos, function, 0, 0, 0), 6);
            }
        }

        // Neither the status checks nor the errors consumed the queued NUL.
        success(&service(&mut dos, 0x4600, console, 0, 0));
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, 0);
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
        assert!(common::output(dos.stdout()).is_empty());
    });
}

#[test]
fn file_input_status_follows_redirection_and_the_shared_cursor() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("IN.TXT", b"\0\x1a".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&service(&mut dos, 0x4500, 0, 0, 0));
        let file = success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        success(&service(&mut dos, 0x4600, file, 0, 0));
        dos.stdin().write_all(b"K").unwrap();
        for byte in [0, 0x1a] {
            for _ in 0..2 {
                assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
                assert_eq!(success(&service(&mut dos, 0x4406, file, 0, 0)) as u8, 0xff);
            }
            assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, byte);
        }
        assert_eq!(success(&service(&mut dos, 0x4201, file, 0, 0)), 2);
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
        assert_eq!(success(&service(&mut dos, 0x4406, file, 0, 0)) as u8, 0);
        // Output status for a file does not check its open access mode.
        assert_eq!(success(&service(&mut dos, 0x4407, file, 0, 0)) as u8, 0xff);
        success(&service(&mut dos, 0x4200, file, 0, 0));
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
        success(&service(&mut dos, 0x4600, console, 0, 0));
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'K');
    });
}

#[test]
fn output_status_and_unattached_devices_report_readiness_and_handle_errors() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for handle in 0..=2 {
            assert_eq!(success(&service(&mut dos, 0x4406, handle, 0, 0)) as u8, 0);
            assert_eq!(
                success(&service(&mut dos, 0x4407, handle, 0, 0)) as u8,
                0xff
            );
        }
        for function in [0x4406, 0x4407] {
            for handle in [3, 4] {
                assert_eq!(success(&service(&mut dos, function, handle, 0, 0)) as u8, 0);
            }
            error(&service(&mut dos, function, 99, 0, 0), 6);
        }
        success(&service(&mut dos, 0x3e00, 1, 0, 0));
        error(&service(&mut dos, 0x4407, 1, 0, 0), 6);
    });
}
