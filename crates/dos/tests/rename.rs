mod common;

use common::{error, path_service, service, success, Backend, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};
use relocade_x86::{CpuState, Segment, SegmentRegister};

fn rename(dos: &mut Dos<Backend>, from: &[u8], to: &[u8]) -> CpuState {
    // Use a distinct ES and cross its offset boundary to exercise both pointers.
    const SEGMENT: u16 = 0x2000;
    const OFFSET: u16 = 0xfffc;
    {
        let mut machine = dos.cpu().context();
        machine.cpu().set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, SEGMENT),
        );
        machine.cpu().set_di(OFFSET);
        for (index, byte) in to.iter().copied().chain([0]).enumerate() {
            let address = u32::from(SEGMENT) * 16 + u32::from(OFFSET.wrapping_add(index as u16));
            machine.memory().write(address, &[byte]).unwrap();
        }
    }
    path_service(dos, from, 0x5600, 0)
}

#[test]
fn rename_moves_contents_and_attributes_using_es_di() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA/OLD.BIN", b"saved".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"SAVES", 0x3900, 0));
        success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
        success(&path_service(&mut dos, b"OLD.BIN", 0x4301, 0x07));

        dos.cpu().context().cpu().set_eflags(0x0a57);
        let cpu = rename(&mut dos, b"old.bin", b"c:..\\saves\\new.bin");
        success(&cpu);
        assert_eq!(cpu.flags.eflags(), 0x0a56);
        assert_eq!(dos.read_file("DATA/OLD.BIN"), None);
        assert_eq!(dos.read_file("SAVES/NEW.BIN").unwrap(), b"saved");
        let cpu = path_service(&mut dos, b"\\SAVES\\NEW.BIN", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x07);
        error(&path_service(&mut dos, b"\\SAVES\\NEW.BIN", 0x3d02, 0), 5);
        error(&path_service(&mut dos, b"OLD.BIN", 0x4e00, 0x06), 18);
        success(&path_service(&mut dos, b"\\SAVES\\NEW.BIN", 0x4e00, 0x06));
        let mut dta = [0; 43];
        dos.cpu()
            .context()
            .memory()
            .read(0x10080, &mut dta)
            .unwrap();
        assert_eq!(dta[21], 0x07);
        assert_eq!(&dta[26..30], &5u32.to_le_bytes());
        assert_eq!(&dta[30..38], b"NEW.BIN\0");

        // Reusing the old name must not inherit the moved file's attributes.
        let handle = success(&path_service(&mut dos, b"OLD.BIN", 0x5b00, 0));
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        let cpu = path_service(&mut dos, b"OLD.BIN", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x20);
        assert_eq!(dos.read_file("SAVES/NEW.BIN").unwrap(), b"saved");
    });
}

#[test]
fn rename_checks_names_paths_and_open_handles_before_moving_a_file() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA/KEEP.BIN", b"keep".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.mount_file("DATA/TAKEN.BIN", b"taken".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
        success(&path_service(&mut dos, b"DIR", 0x3900, 0));
        let unterminated = [b'X'; 128];
        for (from, to, code) in [
            (b"KEEP.BIN".as_slice(), b"TAKEN.BIN".as_slice(), 5),
            (b"KEEP.BIN", b"keep.bin", 5),
            (b"MISSING", b"NEW.BIN", 2),
            (b"MISSING/FILE", b"NEW.BIN", 3),
            (b"KEEP.BIN", b"MISSING/NEW.BIN", 3),
            (b"KEEP.BIN", b"TAKEN.BIN/NEW.BIN", 3),
            (b"KEEP.BIN", b"DIR", 5),
            (b"DIR", b"NEW.BIN", 5),
            (b"KEEP.BIN", b"D:\\NEW.BIN", 17),
            (b"D:\\KEEP.BIN", b"D:\\NEW.BIN", 3),
            (b"*.BIN", b"NEW.BIN", 3),
            (b"KEEP.BIN", b"NEW?.BIN", 3),
            (b"KEEP.BIN", b"1:\\NEW.BIN", 3),
            (b"KEEP.BIN", b"", 3),
            (b"KEEP.BIN", unterminated.as_slice(), 3),
        ] {
            error(&rename(&mut dos, from, to), code);
            assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"keep");
            assert_eq!(dos.read_file("DATA/TAKEN.BIN").unwrap(), b"taken");
            assert_eq!(dos.read_file("DATA/NEW.BIN"), None);
        }

        let handle = success(&path_service(&mut dos, b"KEEP.BIN", 0x3d42, 0));
        let duplicate = success(&service(&mut dos, 0x4500, handle, 0, 0));
        error(&rename(&mut dos, b"KEEP.BIN", b"NEW.BIN"), 5);
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        error(&rename(&mut dos, b"KEEP.BIN", b"NEW.BIN"), 5);
        success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
        success(&rename(&mut dos, b"KEEP.BIN", b"NEW.BIN"));
        assert_eq!(dos.read_file("DATA/KEEP.BIN"), None);
        assert_eq!(dos.read_file("DATA/NEW.BIN").unwrap(), b"keep");
    });
}
