mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};
use relocade_x86::{CpuState, Segment, SegmentRegister};

fn temporary(dos: &mut Dos, directory: &[u8], attributes: u16) -> (CpuState, Vec<u8>) {
    // Keep the output buffer in a separate segment and cross its offset boundary.
    const SEGMENT: u16 = 0x2000;
    const OFFSET: u16 = 0xfffc;
    let mut buffer = vec![0xcc; directory.len() + 1 + 13];
    buffer[..directory.len()].copy_from_slice(directory);
    buffer[directory.len()] = 0;
    {
        let mut machine = dos.cpu().context();
        machine.cpu.set_segment(
            Segment::Ds,
            SegmentRegister::real_mode(Segment::Ds, SEGMENT),
        );
        for (index, byte) in buffer.iter().enumerate() {
            let address = u32::from(SEGMENT) * 16 + u32::from(OFFSET.wrapping_add(index as u16));
            machine.memory.write(address, &[*byte]).unwrap();
        }
    }
    let cpu = service(dos, 0x5a00, 0, attributes, OFFSET);
    let mut machine = dos.cpu().context();
    for (index, byte) in buffer.iter_mut().enumerate() {
        let address = u32::from(SEGMENT) * 16 + u32::from(OFFSET.wrapping_add(index as u16));
        machine
            .memory
            .read(address, std::slice::from_mut(byte))
            .unwrap();
    }
    machine
        .cpu
        .set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x1000));
    (cpu, buffer)
}

fn generated_path(buffer: &[u8]) -> &[u8] {
    let end = buffer.iter().position(|byte| *byte == 0).unwrap();
    assert!(buffer[end + 1..].iter().all(|byte| *byte == 0xcc));
    &buffer[..end]
}

#[test]
fn temporary_files_preserve_collisions_and_return_a_writable_handle_and_path() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        // The name sequence starts at zero; both files and directories occupy names.
        dos.mount_file("TMP/00000000", b"keep".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"TMP/00000001", 0x3900, 0));
        dos.cpu().context().cpu.set_eflags(0x0a57);
        let (cpu, buffer) = temporary(&mut dos, b"c:\\tmp\\", 0x07);
        let handle = success(&cpu);
        assert_eq!(handle, 5);
        assert_eq!(cpu.flags.eflags(), 0x0a56);
        assert_eq!(cpu.registers.edx as u16, 0xfffc);
        assert_eq!(cpu.segments[Segment::Ds].selector, 0x2000);
        let path = generated_path(&buffer);
        assert!(path.starts_with(b"c:\\tmp\\"));
        assert_eq!(path.len(), b"c:\\tmp\\".len() + 8);
        let name = std::str::from_utf8(path).unwrap();
        assert_eq!(dos.read_file(name).unwrap(), b"");
        assert_eq!(dos.read_file("TMP/00000000").unwrap(), b"keep");
        let cpu = path_service(&mut dos, b"TMP/00000001", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x10);
        let cpu = path_service(&mut dos, path, 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x27);

        dos.cpu().context().memory.write(0x10400, b"saved").unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, handle, 5, 0x400)), 5);
        success(&service(&mut dos, 0x4200, handle, 0, 0));
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 5, 0x500)), 5);
        assert_eq!(dos.read_file(name).unwrap(), b"saved");
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        error(&path_service(&mut dos, path, 0x3d02, 0), 5);

        // A directory without a trailing slash receives one.
        let (cpu, second) = temporary(&mut dos, b"TMP", 0);
        let second_handle = success(&cpu);
        assert!(generated_path(&second).starts_with(b"TMP\\"));
        assert_ne!(generated_path(&second)[4..], path[7..]);
        success(&service(&mut dos, 0x3e00, second_handle, 0, 0));
        success(&path_service(&mut dos, b"TMP", 0x3b00, 0));
        // Empty and drive-only paths name the root, even with a different cwd.
        for (directory, prefix) in [(b"".as_slice(), b"\\".as_slice()), (b"c:", b"c:\\")] {
            let (cpu, buffer) = temporary(&mut dos, directory, 0);
            let handle = success(&cpu);
            let path = generated_path(&buffer);
            assert!(path.starts_with(prefix));
            assert_eq!(path.len(), prefix.len() + 8);
            let name = std::str::from_utf8(&path[prefix.len()..]).unwrap();
            assert_eq!(dos.read_file(name).unwrap(), b"");
            assert!(dos.read_file(&format!("TMP/{name}")).is_none());
            success(&service(&mut dos, 0x3e00, handle, 0, 0));
            success(&path_service(&mut dos, path, 0x4300, 0));
        }
        // DOS does not delete temporary files on program exit or replacement.
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(dos.read_file(name).unwrap(), b"saved");
    });
}

#[test]
fn temporary_file_errors_preserve_the_buffer_and_do_not_create_files() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA/KEEP.BIN", b"keep".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        let long_directory = "A".repeat(119); // Adding a separator and name exceeds 127 bytes.
        dos.mount_file(
            &format!("{long_directory}/F"),
            vec![],
            FileAccess::ReadWrite,
        )
        .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for (directory, attributes, code) in [
            (b"MISSING\\".as_slice(), 0, 3),
            (b"DATA/KEEP.BIN", 0, 3),
            (b"D:\\", 0, 3),
            (b"DATA/*", 0, 3),
            (b"DATA\\", 0x10, 5),
            (b"DATA\\", 0x08, 5),
            (long_directory.as_bytes(), 0, 3),
        ] {
            let (cpu, buffer) = temporary(&mut dos, directory, attributes);
            error(&cpu, code);
            assert_eq!(generated_path(&buffer), directory);
        }
        error(&path_service(&mut dos, b"DATA/????????", 0x4e00, 0x06), 18);
        // Occupy all remaining handles without creating any files.
        for handle in 5..20 {
            assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), handle);
        }
        let (cpu, buffer) = temporary(&mut dos, b"DATA\\", 0);
        error(&cpu, 4);
        assert_eq!(generated_path(&buffer), b"DATA\\");
        error(&path_service(&mut dos, b"DATA/????????", 0x4e00, 0x06), 18);
        assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"keep");
        success(&service(&mut dos, 0x3e00, 5, 0, 0));
        let (cpu, buffer) = temporary(&mut dos, b"DATA\\", 0);
        assert_eq!(success(&cpu), 5);
        assert_eq!(
            dos.read_file(std::str::from_utf8(generated_path(&buffer)).unwrap())
                .unwrap(),
            b""
        );
    });
}
