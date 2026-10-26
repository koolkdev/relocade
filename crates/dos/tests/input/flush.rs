use super::{common, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;
use relocade_x86::{ExecutionExit, HostExit};
use std::io::Write;

#[test]
fn flush_then_read_waits_for_new_input_in_the_existing_interrupt_frame() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        for function in [0x01, 0x07, 0x08, 0x0a] {
            dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
            dos.stdin().write_all(b"old\r").unwrap();
            {
                let mut machine = dos.cpu().context();
                machine.memory.write(0x10400, &[8, 0]).unwrap();
                let cpu = &mut machine.cpu;
                cpu.set_ax(0x0c00 | function);
                cpu.set_bx(0x1234);
                cpu.set_dx(0x400);
                cpu.set_eflags(0x643);
            }
            assert_eq!(
                dos.cpu().run(10).unwrap(),
                ExecutionExit::Host(HostExit::Deferred)
            );
            assert!(common::output(dos.stdout()).is_empty());
            assert_eq!(dos.cpu().context().cpu.sp(), 0xfff8);
            // Calling run again cannot flush input or push another interrupt frame.
            assert_eq!(
                dos.cpu().run(10).unwrap(),
                ExecutionExit::Host(HostExit::Deferred)
            );
            dos.stdin()
                .write_all(if function == 0x0a { b"K\r" } else { b"K" })
                .unwrap();
            assert_eq!(
                dos.cpu().run_async(10).await.unwrap(),
                ExecutionExit::BudgetExhausted
            );
            {
                let machine = dos.cpu().context();
                if function == 0x0a {
                    let mut buffer = [0; 4];
                    machine.memory.read(0x10400, &mut buffer).unwrap();
                    assert_eq!(&buffer, b"\x08\x01K\r");
                } else {
                    assert_eq!(machine.cpu.al(), b'K');
                }
                let cpu = &machine.cpu;
                assert_eq!(cpu.bx(), 0x1234);
                assert_eq!(cpu.dx(), 0x400);
                assert_eq!(cpu.eflags(), 0x643);
                assert_eq!(cpu.sp(), 0xfffe);
                assert_eq!(cpu.eip(), 0x102);
            }
            let expected = match function {
                0x01 => b"K".as_slice(),
                0x0a => b"K\r",
                _ => b"",
            };
            assert_eq!(common::output(dos.stdout()), expected);
        }
    });
}

#[test]
fn flush_only_and_direct_io_leave_output_and_buffered_handle_input_alone() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        service(&mut dos, 0x0200, 0, 0, u16::from(b'P'));
        dos.stdin().write_all(b"old\r").unwrap();
        dos.cpu().context().cpu.set_eflags(0x603);
        let cpu = service(&mut dos, 0x0c06, 0, 0, 0xff);
        assert_eq!(cpu.registers.eax as u8, 0);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert_eq!(common::output(dos.stdout()), b"P");
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);

        dos.stdin().write_all(b"old").unwrap();
        service(&mut dos, 0x0c06, 0, 0, u16::from(b'!'));
        assert_eq!(common::output(dos.stdout()), b"!");
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
        for function in [0, 0x0c, 0xff] {
            dos.stdin().write_all(b"old").unwrap();
            dos.cpu().context().cpu.set_eflags(0x603);
            let cpu = service(&mut dos, 0x0c00 | function, 0, 0, 0);
            assert_eq!(cpu.registers.eax as u8, 0);
            assert_eq!(cpu.flags.eflags(), 0x603);
            assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
        }

        dos.stdin().write_all(b"abc\rqueued").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        service(&mut dos, 0x0c00, 0, 0, 0);
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 4);
        let mut buffer = [0; 4];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut buffer)
            .unwrap();
        assert_eq!(&buffer, b"bc\r\n");

        dos.stdin().write_all(b"\x1a\rqueued").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
        service(&mut dos, 0x0c00, 0, 0, 0);
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x8083);
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0);
    });
}

#[test]
fn flush_redirected_stdin_preserves_file_position_and_console_queue() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("IN.TXT", b"AB".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&service(&mut dos, 0x4500, 0, 0, 0));
        let file = success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        success(&service(&mut dos, 0x4600, file, 0, 0));
        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'A');
        service(&mut dos, 0x0c00, 0, 0, 0);
        assert_eq!(success(&service(&mut dos, 0x4201, file, 0, 0)), 1);
        assert_eq!(service(&mut dos, 0x0c07, 0, 0, 0).registers.eax as u8, b'B');
        assert_eq!(success(&service(&mut dos, 0x4201, file, 0, 0)), 2);
        success(&service(&mut dos, 0x4600, console, 0, 0));
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'K');
        // A failed flush does not prevent a selected output service from running.
        success(&service(&mut dos, 0x3e00, 0, 0, 0));
        service(&mut dos, 0x0c06, 0, 0, u16::from(b'!'));
        assert_eq!(common::output(dos.stdout()), b"!");
    });
}
