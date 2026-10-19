mod common;

use common::{error, path_service, service, success, Backend, SERVICE_PROGRAM};
use wasm86_dos::{Dos, DosState, FileAccess};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

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
    // CON is a character device with input/output attributes and no input EOF.
    for handle in 0..=2 {
        assert_eq!(information(&mut dos, handle), 0x00c3);
    }
    assert_eq!(dos.runtime().machine().cpu().eflags(), 0x0a56);
    for handle in [3, 4] {
        assert_eq!(information(&mut dos, handle), 0x00c0); // AUX and PRN.
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
    assert_eq!(information(&mut dos, 1), 0x00c3); // Restored stdout is a device.
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ioctl_reports_the_object_and_write_state_behind_each_handle() {
    pollster::block_on(check_device_information());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ioctl_reports_the_object_and_write_state_behind_each_handle() {
    check_device_information().await;
}
