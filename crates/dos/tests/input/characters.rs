use super::{common, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;
use relocade_x86::{ExecutionExit, HostExit};
use std::io::Write;

#[test]
fn character_reads_share_the_async_path_and_echo_only_for_ah01() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        for function in [0x01, 0x08] {
            dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
            {
                let mut machine = dos.cpu().context();
                let cpu = &mut machine.cpu;
                cpu.set_ax(function << 8);
                cpu.set_bx(0x1234);
                cpu.set_eflags(0x643);
            }
            assert_eq!(
                dos.cpu().run(10).unwrap(),
                ExecutionExit::Host(HostExit::Deferred)
            );
            assert!(common::output(dos.stdout()).is_empty());
            dos.stdin().write_all(b"KZ").unwrap();
            assert_eq!(
                dos.cpu().run_async(10).await.unwrap(),
                ExecutionExit::BudgetExhausted
            );
            {
                let machine = dos.cpu().context();
                let cpu = &machine.cpu;
                assert_eq!(cpu.ax(), (function << 8) | u16::from(b'K'));
                assert_eq!(cpu.bx(), 0x1234);
                assert_eq!(cpu.eflags(), 0x643);
                assert_eq!(cpu.eip(), 0x102);
                assert_eq!(cpu.sp(), 0xfffe);
            }
            // The second queued byte returns immediately through the same handler.
            let cpu = service(&mut dos, function << 8, 0, 0, 0);
            assert_eq!(cpu.registers.eax as u8, b'Z');
            assert_eq!(
                common::output(dos.stdout()),
                if function == 1 { b"KZ".as_slice() } else { b"" }
            );
        }
    });
}

#[test]
fn character_input_and_echo_follow_separate_standard_handle_redirections() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("IN.TXT", b"A\t\x1a".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&service(&mut dos, 0x4500, 0, 0, 0));
        let input = success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        let output = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));
        success(&service(&mut dos, 0x4600, input, 0, 0));
        success(&service(&mut dos, 0x4600, output, 1, 0));
        dos.stdin().write_all(b"Q").unwrap();
        for byte in b"A\t\x1a" {
            assert_eq!(
                service(&mut dos, 0x0100, 0, 0, 0).registers.eax as u8,
                *byte
            );
        }
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"A       \x1a");
        assert!(common::output(dos.stdout()).is_empty());
        success(&service(&mut dos, 0x4600, console, 0, 0));
        assert_eq!(service(&mut dos, 0x0800, 0, 0, 0).registers.eax as u8, b'Q');
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"A       \x1a");
    });
}
