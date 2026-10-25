mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess, MountError};
use relocade_x86::{CpuState, ExecutionExit};

async fn check_file_program() {
    let mut program = vec![
        0xb8, 0, 0x3d, // MOV AX,3D00: open read-only
        0xba, 0x40, 1, // MOV DX,0140: pathname
        0xcd, 0x21, 0x72, 0x24, // JC failure
        0x89, 0xc3, // MOV BX,AX: file handle
        0x89, 0xde, // MOV SI,BX: preserve it for close
        0xb9, 0x40, 0, // MOV CX,64: read capacity
        0xba, 0, 2, // MOV DX,0200: buffer
        0xb4, 0x3f, 0xcd, 0x21, // Read file.
        0x72, 0x14, // JC failure
        0x89, 0xc1, // MOV CX,AX: bytes actually read
        0xbb, 1, 0, // MOV BX,1: stdout
        0xb4, 0x40, 0xcd, 0x21, // Write buffer.
        0x89, 0xf3, // MOV BX,SI: original handle
        0xb4, 0x3e, 0xcd, 0x21, // Close file.
        0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
        0xb8, 0xff, 0x4c, 0xcd, 0x21, // failure: exit 255.
    ];
    program.resize(0x40, 0);
    program.extend_from_slice(b"data/hello.txt\0");
    let mut dos = common::dos(&program).await;
    dos.mount_file(
        r"C:\DATA\HELLO.TXT",
        b"file\r\n".to_vec(),
        FileAccess::ReadOnly,
    )
    .unwrap();
    assert_eq!(dos.cpu().unwrap().run(100).await, ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(0)));
    assert_eq!(common::output(dos.stdout()), b"file\r\n");
}

fn open(dos: &mut Dos, path: &[u8], mode: u8) -> CpuState {
    path_service(dos, path, 0x3d00 | u16::from(mode), 0)
}

fn create(dos: &mut Dos, path: &[u8], attributes: u16) -> CpuState {
    path_service(dos, path, 0x3c00, attributes)
}

fn buffer(dos: &mut Dos) -> [u8; 8] {
    let mut bytes = [0; 8];
    dos.cpu()
        .unwrap()
        .context()
        .memory()
        .read(0x10200, &mut bytes)
        .unwrap();
    bytes
}

async fn check_handles() {
    let mut dos = common::dos(SERVICE_PROGRAM).await;
    for path in ["", "C:\\", r"D:\X.BIN", "X?.BIN", "X\0.BIN", "é.bin"] {
        assert_eq!(
            dos.mount_file(path, vec![], FileAccess::ReadOnly),
            Err(MountError::InvalidPath)
        );
    }
    dos.mount_file("X.BIN", b"abcd".to_vec(), FileAccess::ReadOnly)
        .unwrap();

    error(&open(&mut dos, b"MISSING.BIN", 0), 2);
    error(&open(&mut dos, b"MISSING/X.BIN", 0), 3);
    error(&open(&mut dos, b"X.BIN", 2), 5); // Read/write mode on a read-only mount.
    error(&open(&mut dos, b"X.BIN", 3), 12); // Invalid access mode.
    error(&open(&mut dos, b"X.BIN", 0x50), 12); // Invalid sharing mode.
    assert_eq!(success(&open(&mut dos, b"c:/./x.bin", 0)), 5);
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0x40)), 6); // Deny-none sharing.

    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 2, 0x200)), 2);
    assert_eq!(&buffer(&mut dos)[..2], b"ab");
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 1, 0x200)), 1);
    assert_eq!(buffer(&mut dos)[0], b'a'); // Independent cursor.
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 0, 0x200)), 0);
    dos.cpu()
        .unwrap()
        .context()
        .memory()
        .write(0x10200, b"!!!!!!!!")
        .unwrap();
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 8, 0x200)), 2);
    assert_eq!(buffer(&mut dos), *b"cd!!!!!!"); // Short read preserves the buffer tail.
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 8, 0x200)), 0); // EOF.
    error(&service(&mut dos, 0x4000, 5, 1, 0x200), 5);
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    error(&service(&mut dos, 0x3e00, 5, 0, 0), 6);
    error(&service(&mut dos, 0x3f00, 5, 0, 0x200), 6);
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 5); // Reuse a closed slot.

    dos.mount_file("X.BIN", b"new".to_vec(), FileAccess::ReadOnly)
        .unwrap();
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 8, 0x200)), 3);
    assert_eq!(&buffer(&mut dos)[..3], b"bcd"); // An existing handle keeps its bytes.
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 7);
    assert_eq!(success(&service(&mut dos, 0x3f00, 7, 8, 0x200)), 3);
    assert_eq!(&buffer(&mut dos)[..3], b"new");

    for handle in 5..=7 {
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }
    error(&service(&mut dos, 0x3f00, 6, 1, 0x200), 6); // Closed handle is invalid.
    for handle in 5..20 {
        assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), handle);
    }
    error(&open(&mut dos, b"X.BIN", 0), 4);
    success(&service(&mut dos, 0x3e00, 10, 0, 0));
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 10);

    for handle in 5..20 {
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0x10)), 5); // Deny other reads.
    error(&open(&mut dos, b"X.BIN", 0), 5);
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 5);
}

async fn check_standard_handles() {
    let mut dos = common::dos(SERVICE_PROGRAM).await;
    dos.mount_file("X.BIN", b"file".to_vec(), FileAccess::ReadOnly)
        .unwrap();

    success(&service(&mut dos, 0x3e00, 1, 0, 0)); // Close stdout.
    error(&service(&mut dos, 0x3e00, 1, 0, 0), 6);
    error(&service(&mut dos, 0x4000, 1, 1, 0x200), 6);
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 1);
    assert_eq!(success(&service(&mut dos, 0x3f00, 1, 4, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"file");
    error(&service(&mut dos, 0x4000, 1, 4, 0x200), 5); // Slot 1 now holds a read-only file.
    assert_eq!(common::output(dos.stdout()), b"");
    assert_eq!(success(&service(&mut dos, 0x4000, 2, 4, 0x200)), 4);
    assert_eq!(common::output(dos.stderr()), b"file"); // Closing stdout leaves stderr open.

    for handle in 0..5 {
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }
    error(&service(&mut dos, 0x4000, 2, 1, 0x200), 6);
    for handle in 0..20 {
        assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), handle);
    }
    error(&open(&mut dos, b"X.BIN", 0), 4);
}

#[test]
fn standard_handles_can_be_closed_and_reused() {
    pollster::block_on(check_standard_handles());
}

fn seek(dos: &mut Dos, handle: u16, origin: u8, offset: i32) -> CpuState {
    service(
        dos,
        0x4200 | u16::from(origin),
        handle,
        (offset >> 16) as u16,
        offset as u16,
    )
}

fn position(cpu: &CpuState) -> u32 {
    u32::from(success(cpu)) | ((cpu.registers.edx & 0xffff) << 16)
}

fn write(dos: &mut Dos, handle: u16, bytes: &[u8]) -> CpuState {
    dos.cpu()
        .unwrap()
        .context()
        .memory()
        .write(0x10200, bytes)
        .unwrap();
    service(dos, 0x4000, handle, bytes.len() as u16, 0x200)
}

async fn check_writes() {
    let mut dos = common::dos(SERVICE_PROGRAM).await;
    dos.mount_file("SAVE.BIN", b"abcdef".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    assert_eq!(dos.read_file("MISSING.BIN"), None);
    assert_eq!(dos.read_file("D:\\SAVE.BIN"), None);

    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x42)), 5); // Read/write, deny none.
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x40)), 6); // Read only.
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x41)), 7); // Write only.
    assert_eq!(success(&write(&mut dos, 5, b"XY")), 2);
    assert_eq!(dos.read_file("save.bin").unwrap(), b"XYcdef");
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 2, 0x200)), 2);
    assert_eq!(&buffer(&mut dos)[..2], b"XY"); // Already-open reader sees the write.
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), 2); // Its cursor remains independent.
    error(&service(&mut dos, 0x3f00, 7, 1, 0x200), 5);
    error(&write(&mut dos, 6, b"!"), 5);
    error(&write(&mut dos, 6, b""), 5); // Read-only handles cannot truncate.
    assert_eq!(success(&write(&mut dos, 7, b"Z")), 1);

    assert_eq!(position(&seek(&mut dos, 5, 0, 8)), 8);
    assert_eq!(success(&write(&mut dos, 5, b"Q")), 1);
    assert_eq!(dos.read_file("SAVE.BIN").unwrap(), b"ZYcdef\0\0Q");
    assert_eq!(position(&seek(&mut dos, 5, 0, 3)), 3);
    assert_eq!(success(&write(&mut dos, 5, b"")), 0);
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), 3);
    assert_eq!(dos.read_file("SAVE.BIN").unwrap(), b"ZYc");
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 4, 0x200)), 1);
    assert_eq!(buffer(&mut dos)[0], b'c');
    assert_eq!(position(&seek(&mut dos, 5, 0, 6)), 6);
    assert_eq!(success(&write(&mut dos, 5, b"")), 0);
    assert_eq!(dos.read_file("SAVE.BIN").unwrap(), b"ZYc\0\0\0");

    assert_eq!(position(&seek(&mut dos, 5, 0, 0)), 0);
    assert_eq!(position(&seek(&mut dos, 5, 1, -1)), u32::MAX);
    error(&write(&mut dos, 5, b"!"), 5);
    assert_eq!(position(&seek(&mut dos, 5, 0, i32::MAX)), 0x7fff_ffff);
    assert_eq!(position(&seek(&mut dos, 5, 1, i32::MAX)), 0xffff_fffe);
    error(&write(&mut dos, 5, b"XX"), 5); // The resulting size cannot fit in 32 bits.
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), 0xffff_fffe);
    assert_eq!(dos.read_file("SAVE.BIN").unwrap(), b"ZYc\0\0\0");
    for handle in 5..=7 {
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }

    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x22)), 5); // Deny other writes.
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x40)), 6);
    error(&open(&mut dos, b"SAVE.BIN", 0x41), 5);
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x31)), 5); // Deny other reads.
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x41)), 6);
    error(&open(&mut dos, b"SAVE.BIN", 0x40), 5);
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x21)), 7); // Same-process existing writers are allowed.
    error(&open(&mut dos, b"SAVE.BIN", 0x41), 5); // Later writers are denied.
    success(&service(&mut dos, 0x3e00, 7, 0, 0));
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    success(&service(&mut dos, 0x3e00, 5, 0, 0));

    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x40)), 5);
    error(&open(&mut dos, b"SAVE.BIN", 0x10), 5); // Deny-both includes this process.
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x30)), 6); // Existing reader is allowed.
    error(&open(&mut dos, b"SAVE.BIN", 0x40), 5); // Later readers are denied.
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    success(&service(&mut dos, 0x3e00, 5, 0, 0));

    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 2)), 5); // Compatibility mode.
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 1)), 6); // Same process may reopen.
    error(&open(&mut dos, b"SAVE.BIN", 0x40), 5); // Cannot mix with explicit sharing.
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&open(&mut dos, b"SAVE.BIN", 0x42)), 5);
    error(&open(&mut dos, b"SAVE.BIN", 0), 5);

    common::terminate(&mut dos);
    dos.launch_com(SERVICE_PROGRAM, Default::default())
        .await
        .unwrap();
    assert_eq!(dos.read_file("SAVE.BIN").unwrap(), b"ZYc\0\0\0");
    error(&write(&mut dos, 5, b"!"), 6); // Guest termination closed the handle.
}

async fn check_seek() {
    let mut dos = common::dos(SERVICE_PROGRAM).await;
    let mut contents = vec![0; 0x10008];
    contents[..4].copy_from_slice(b"head");
    contents[0x10000..].copy_from_slice(b"taildata");
    dos.mount_file("RECORDS.BIN", contents, FileAccess::ReadOnly)
        .unwrap();

    assert_eq!(success(&open(&mut dos, b"RECORDS.BIN", 0)), 5);
    assert_eq!(success(&open(&mut dos, b"RECORDS.BIN", 0)), 6);

    // Seek-to-end reports the size in DX:AX, including the high word.
    assert_eq!(position(&seek(&mut dos, 5, 2, 0)), 0x10008);
    assert_eq!(position(&seek(&mut dos, 5, 0, 0x10000)), 0x10000);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 4, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"tail");
    assert_eq!(position(&seek(&mut dos, 5, 1, -2)), 0x10002);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 4, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"ilda");
    assert_eq!(position(&seek(&mut dos, 5, 2, -4)), 0x10004);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 8, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"data");
    // Another open handle retains its own position.
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 4, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"head");

    assert_eq!(position(&seek(&mut dos, 5, 2, 10)), 0x10012);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 8, 0x200)), 0);
    assert_eq!(&buffer(&mut dos)[..4], b"head"); // EOF leaves the buffer untouched.
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), 0x10012); // Retain position past EOF.
    error(&seek(&mut dos, 5, 3, 0), 1);
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), 0x10012); // Failed seek is unchanged.
    error(&seek(&mut dos, 20, 0, 0), 6);

    // The cursor uses all 32 bits and wraps at the 4 GiB boundary.
    assert_eq!(position(&seek(&mut dos, 5, 0, i32::MAX)), 0x7fff_ffff);
    assert_eq!(position(&seek(&mut dos, 5, 1, 1)), 0x8000_0000);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 1, 0x200)), 0);
    assert_eq!(position(&seek(&mut dos, 5, 1, i32::MAX)), u32::MAX);
    assert_eq!(position(&seek(&mut dos, 5, 1, 1)), 0);
    assert_eq!(position(&seek(&mut dos, 5, 0, -1)), u32::MAX); // Absolute CX:DX is unsigned.
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), u32::MAX);

    // Negative displacements wrap to the same cursor value as an absolute seek.
    assert_eq!(position(&seek(&mut dos, 5, 0, 0)), 0);
    assert_eq!(position(&seek(&mut dos, 5, 1, -1)), u32::MAX);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 1, 0x200)), 0);
    assert_eq!(position(&seek(&mut dos, 5, 2, -0x10009)), u32::MAX);
    assert_eq!(position(&seek(&mut dos, 5, 1, 1)), 0);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 4, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"head");
}

async fn check_save_program() {
    let mut program = vec![
        0xb4, 0x3c, // MOV AH,3C: create file
        0x31, 0xc9, // XOR CX,CX: normal attributes
        0xba, 0x40, 1, // MOV DX,0140: pathname
        0xcd, 0x21, // INT 21h
        0x89, 0xc3, // MOV BX,AX: new handle
        0xb9, 5, 0, // MOV CX,5: byte count
        0xba, 0x50, 1, // MOV DX,0150: data
        0xb4, 0x40, 0xcd, 0x21, // Write file.
        0xb4, 0x3e, 0xcd, 0x21, // Close file.
        0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
    ];
    program.resize(0x40, 0);
    program.extend_from_slice(b"SAVE.DAT\0");
    program.resize(0x50, 0);
    program.extend_from_slice(b"saved");
    let mut dos = common::dos(&program).await;

    assert_eq!(dos.cpu().unwrap().run(100).await, ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(0)));
    assert_eq!(dos.read_file(r"C:\save.dat").unwrap(), b"saved");
}

async fn check_creation() {
    let mut dos = common::dos(SERVICE_PROGRAM).await;
    dos.mount_file(
        "DATA/KEEP.BIN",
        b"old contents".to_vec(),
        FileAccess::ReadWrite,
    )
    .unwrap();
    dos.mount_file("READONLY.BIN", b"keep".to_vec(), FileAccess::ReadOnly)
        .unwrap();

    error(&create(&mut dos, b"MISSING/NEW.BIN", 0), 3);
    error(&create(&mut dos, b"DATA", 0), 5); // A directory occupies this name.
    error(&create(&mut dos, b"INVALID.BIN", 0x10), 5); // Directory attribute.
    error(&create(&mut dos, b"INVALID.BIN", 0x100), 5); // Unsupported attribute.
    assert_eq!(dos.read_file("INVALID.BIN"), None);
    error(&create(&mut dos, b"READONLY.BIN", 0), 5);
    assert_eq!(dos.read_file("READONLY.BIN").unwrap(), b"keep");
    for path in [
        "DATA/KEEP.BIN/",
        "DATA/KEEP.BIN\\",
        "DATA/KEEP.BIN/.",
        "DATA/KEEP.BIN/..",
    ] {
        error(&create(&mut dos, path.as_bytes(), 0), 3);
        assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"old contents");
    }

    assert_eq!(success(&open(&mut dos, b"DATA/KEEP.BIN", 0)), 5);
    assert_eq!(position(&seek(&mut dos, 5, 0, 2)), 2);
    assert_eq!(success(&create(&mut dos, b"data\\keep.bin", 0x20)), 6); // Archive attribute.
    assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"");
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 1, 0x200)), 0);
    assert_eq!(position(&seek(&mut dos, 5, 1, 0)), 2); // Old handle sees truncation, keeps cursor.
    assert_eq!(success(&write(&mut dos, 6, b"fresh")), 5);
    assert_eq!(position(&seek(&mut dos, 5, 0, 0)), 0);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 5, 0x200)), 5);
    assert_eq!(&buffer(&mut dos)[..5], b"fresh");
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    success(&service(&mut dos, 0x3e00, 6, 0, 0));
    assert_eq!(success(&open(&mut dos, b"DATA/KEEP.BIN", 0x12)), 5); // Deny all.
    error(&create(&mut dos, b"DATA/KEEP.BIN", 0), 5);
    assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"fresh");
    success(&service(&mut dos, 0x3e00, 5, 0, 0));

    // Hidden and system flags, alone or combined with archive, allow file access.
    for attributes in [0x02, 0x04, 0x26] {
        assert_eq!(success(&create(&mut dos, b"FLAGS.BIN", attributes)), 5);
        assert_eq!(success(&write(&mut dos, 5, b"flags")), 5);
        success(&service(&mut dos, 0x3e00, 5, 0, 0));
        assert_eq!(success(&open(&mut dos, b"FLAGS.BIN", 2)), 5);
        success(&service(&mut dos, 0x3e00, 5, 0, 0));
    }

    // Read-only may combine with hidden/system; the initial handle remains read/write.
    assert_eq!(success(&create(&mut dos, b"LOCKED.BIN", 0x07)), 5);
    assert_eq!(success(&write(&mut dos, 5, b"protected")), 9);
    assert_eq!(position(&seek(&mut dos, 5, 0, 0)), 0);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 3, 0x200)), 3);
    assert_eq!(&buffer(&mut dos)[..3], b"pro");
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    error(&open(&mut dos, b"LOCKED.BIN", 2), 5);
    error(&create(&mut dos, b"LOCKED.BIN", 0), 5);
    assert_eq!(dos.read_file("LOCKED.BIN").unwrap(), b"protected");

    for handle in 5..20 {
        assert_eq!(success(&open(&mut dos, b"DATA/KEEP.BIN", 0)), handle);
    }
    error(&create(&mut dos, b"DATA/KEEP.BIN", 0), 4);
    error(&create(&mut dos, b"NEW.BIN", 0), 4);
    assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"fresh");
    assert_eq!(dos.read_file("NEW.BIN"), None); // Failure does not insert an empty file.
    success(&service(&mut dos, 0x3e00, 8, 0, 0));
    assert_eq!(success(&create(&mut dos, b"DATA/KEEP.BIN", 0)), 8);
    assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"");

    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&create(&mut dos, b"DATA/NEW.BIN", 0)), 5);
    assert_eq!(dos.read_file("DATA/NEW.BIN"), Some(Vec::new()));
    assert_eq!(dos.read_file("LOCKED.BIN").unwrap(), b"protected");
}

#[test]
fn program_reads_a_mounted_file_and_prints_it() {
    pollster::block_on(check_file_program());
}

#[test]
fn file_handles_report_counts_errors_and_reuse_closed_slots() {
    pollster::block_on(check_handles());
}

#[test]
fn seeking_selects_records_and_reports_file_positions() {
    pollster::block_on(check_seek());
}

#[test]
fn writable_files_share_contents_and_preserve_access_rules() {
    pollster::block_on(check_writes());
}

#[test]
fn program_creates_a_save_file_for_the_host() {
    pollster::block_on(check_save_program());
}

#[test]
fn creation_checks_attributes_sharing_and_handle_capacity_before_truncating() {
    pollster::block_on(check_creation());
}
