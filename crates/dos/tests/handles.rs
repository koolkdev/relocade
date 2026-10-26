mod common;
#[path = "handles/devices.rs"]
mod devices;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;

async fn check_shared_file_handles() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA.BIN", b"abcdef".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0)), 5);
    assert_eq!(success(&service(&mut dos, 0x4500, 5, 0, 0)), 6);
    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d40, 0)), 7);
    for (handle, expected) in [(5, b"ab"), (6, b"cd"), (7, b"ab")] {
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 2, 0x200)), 2);
        let mut bytes = [0; 2];
        dos.cpu()
            .context()
            .memory
            .read(0x10200, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, expected);
    }
    // Seek and write through either duplicate move the same cursor.
    assert_eq!(success(&service(&mut dos, 0x4200, 6, 0, 1)), 1);
    dos.cpu().context().memory.write(0x10200, b"Z").unwrap();
    assert_eq!(success(&service(&mut dos, 0x4000, 5, 1, 0x200)), 1);
    assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 2);
    assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"aZcdef");
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 2, 0x200)), 2);
    let mut bytes = [0; 2];
    dos.cpu()
        .context()
        .memory
        .read(0x10200, &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"cd"); // Closing the source keeps the duplicate alive.
    assert_eq!(success(&service(&mut dos, 0x4500, 7, 0, 0)), 5);
    error(&service(&mut dos, 0x4000, 5, 1, 0x200), 5); // Access rights survive duplication.

    // Sharing restrictions remain until the last duplicate closes.
    common::terminate(&mut dos);
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d22, 0)), 5);
    assert_eq!(success(&service(&mut dos, 0x4500, 5, 0, 0)), 6);
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    error(&path_service(&mut dos, b"DATA.BIN", 0x3d41, 0), 5);
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    assert_eq!(success(&path_service(&mut dos, b"DATA.BIN", 0x3d41, 0)), 5);
}

async fn check_redirection_and_capacity() {
    let mut dos = common::dos().await;
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), 5); // Save stdout.
    assert_eq!(success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0)), 6);
    success(&service(&mut dos, 0x4600, 6, 1, 0)); // Redirect stdout to the file.
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    error(&service(&mut dos, 0x4600, 6, 1, 0), 6); // Invalid source leaves stdout intact.
    error(&service(&mut dos, 0x4600, 5, 20, 0), 6);
    dos.cpu().context().memory.write(0x10200, b"file").unwrap();
    assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x200)), 4);
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"file");
    assert!(common::output(dos.stdout()).is_empty());
    success(&service(&mut dos, 0x4600, 5, 1, 0)); // Restore stdout.
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x200)), 4);
    assert_eq!(common::output(dos.stdout()), b"file");
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"file");

    // Forced duplication needs no vacant slot, including when source == destination.
    for handle in 5..20 {
        assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), handle);
    }
    error(&service(&mut dos, 0x4500, 1, 0, 0), 4);
    error(&service(&mut dos, 0x4500, u16::MAX, 0, 0), 6);
    success(&service(&mut dos, 0x4600, 1, 19, 0));
    success(&service(&mut dos, 0x4600, 1, 1, 0));
    success(&service(&mut dos, 0x3e00, 19, 0, 0));
    success(&service(&mut dos, 0x4600, 1, 19, 0)); // A closed destination is valid too.
    assert_eq!(success(&service(&mut dos, 0x4000, 19, 4, 0x200)), 4);
    assert_eq!(common::output(dos.stdout()), b"file");

    common::terminate(&mut dos);
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    error(&service(&mut dos, 0x4500, 19, 0, 0), 6);
    assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), 5);
}

#[test]
fn duplicate_file_handles_share_positions_rights_and_lifetime() {
    pollster::block_on(check_shared_file_handles());
}

#[test]
fn forced_handle_duplication_redirects_output_and_validates_slots() {
    pollster::block_on(check_redirection_and_capacity());
}
