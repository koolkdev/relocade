mod common;

use common::{error, path_service, service, success, Backend, SERVICE_PROGRAM};
use relocade_dos::{Dos, DosState, FileAccess};
use relocade_x86::{ExecutionExit, Segment, StoredSegment};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn dta(dos: &mut Dos<Backend<DosState>>, address: u32) -> [u8; 43] {
    let mut data = [0; 43];
    dos.runtime()
        .machine()
        .memory()
        .read(address, &mut data)
        .unwrap();
    data
}

fn found_name(data: &[u8; 43]) -> &str {
    let bytes = &data[30..];
    let end = bytes.iter().position(|byte| *byte == 0).unwrap();
    std::str::from_utf8(&bytes[..end]).unwrap()
}

fn names(dos: &mut Dos<Backend<DosState>>, pattern: &[u8], attributes: u16) -> Vec<String> {
    let mut cpu = path_service(dos, pattern, 0x4e00, attributes);
    let mut names = Vec::new();
    while cpu.flags.eflags() & 1 == 0 {
        names.push(found_name(&dta(dos, 0x10080)).to_owned());
        cpu = service(dos, 0x4f00, 0, 0, 0);
    }
    error(&cpu, 18);
    names
}

async fn check_searches() {
    let mut dos = common::dos().await;
    for name in [
        "DATA/A.DAT",
        "DATA/AB.DAT",
        "DATA/B.TXT",
        "DATA/PLAIN",
        "DATA/SUB/CHILD.DAT",
        "ROOT.DAT",
        "DATA/TOO-LONG-NAME.DAT",
    ] {
        dos.mount_file(name, vec![1, 2, 3], FileAccess::ReadOnly)
            .unwrap();
    }
    dos.mount_file("BOUNDARY/ABCDEFGH.DAT", vec![], FileAccess::ReadOnly)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    let cpu = service(&mut dos, 0x2f00, 0, 0, 0);
    assert_eq!(cpu.segments[Segment::Es].selector, 0x1000);
    assert_eq!(cpu.registers.ebx as u16, 0x80);
    error(&service(&mut dos, 0x4f00, 0, 0, 0), 18); // No search has started.
    error(&path_service(&mut dos, b"MISSING/*.DAT", 0x4e00, 0), 3);
    success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
    assert_eq!(names(&mut dos, b"a?.dat", 0), ["A.DAT", "AB.DAT"]);
    assert_eq!(
        names(&mut dos, b"*.*", 0),
        ["A.DAT", "AB.DAT", "B.TXT", "PLAIN"]
    );
    assert_eq!(names(&mut dos, b"*.", 0), ["PLAIN"]);
    assert_eq!(
        names(&mut dos, b"*.*", 0x10),
        [".", "..", "A.DAT", "AB.DAT", "B.TXT", "PLAIN", "SUB"]
    );
    assert_eq!(names(&mut dos, b"C:*.DAT", 0), ["A.DAT", "AB.DAT"]);
    assert_eq!(names(&mut dos, b"C:\\*.DAT", 0), ["ROOT.DAT"]);
    assert!(names(&mut dos, b"*.NONE", 0).is_empty());
    assert_eq!(
        names(&mut dos, b"C:\\BOUNDARY\\ABCDEFGH*.DAT", 0),
        ["ABCDEFGH.DAT"]
    );
    assert_eq!(
        names(&mut dos, b"C:\\BOUNDARY\\*.DAT*", 0),
        ["ABCDEFGH.DAT"]
    );

    for attributes in [0x08, 0x09, 0x28] {
        assert!(names(&mut dos, b"*.*", attributes).is_empty()); // No volume label.
    }

    success(&path_service(&mut dos, b"*.DAT", 0x4e00, 0));
    let saved = dta(&mut dos, 0x10080);
    assert_eq!(found_name(&saved), "A.DAT");
    // An intervening search and chdir do not affect a saved DTA's continuation.
    assert_eq!(names(&mut dos, b"*.TXT", 0), ["B.TXT"]);
    success(&path_service(&mut dos, b"\\", 0x3b00, 0));
    dos.runtime()
        .machine()
        .memory()
        .write(0x10600, &saved)
        .unwrap();
    dos.runtime()
        .machine()
        .cpu()
        .set_segment(Segment::Ds, StoredSegment::real_mode(Segment::Ds, 0x1010))
        .unwrap();
    // Preserve a non-normalized segment:offset pair exactly in AH=2Fh.
    let cpu = service(&mut dos, 0x1a00, 0, 0, 0x500);
    assert_eq!(cpu.flags.eflags() & 1, 0);
    let cpu = service(&mut dos, 0x2f00, 0, 0, 0);
    assert_eq!(cpu.segments[Segment::Es].selector, 0x1010);
    assert_eq!(cpu.registers.ebx as u16, 0x500);
    success(&service(&mut dos, 0x4f00, 0, 0, 0));
    assert_eq!(found_name(&dta(&mut dos, 0x10600)), "AB.DAT");
    error(&service(&mut dos, 0x4f00, 0, 0, 0), 18);
    error(&service(&mut dos, 0x4f00, 0, 0, 0), 18);
    // Restore the same continuation to replay it independently.
    dos.runtime()
        .machine()
        .memory()
        .write(0x10600, &saved)
        .unwrap();
    success(&service(&mut dos, 0x4f00, 0, 0, 0));
    assert_eq!(found_name(&dta(&mut dos, 0x10600)), "AB.DAT");
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    let cpu = service(&mut dos, 0x2f00, 0, 0, 0);
    assert_eq!(cpu.segments[Segment::Es].selector, 0x1000);
    assert_eq!(cpu.registers.ebx as u16, 0x80);
}

async fn check_attributes() {
    let mut dos = common::dos().await;
    dos.mount_file("NORMAL.DAT", b"abc".to_vec(), FileAccess::ReadOnly)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    for (name, attributes) in [
        (b"HIDDEN.DAT".as_slice(), 2),
        (b"SYSTEM.DAT", 4),
        (b"BOTH.DAT", 6),
    ] {
        let handle = success(&path_service(&mut dos, name, 0x3c00, attributes));
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }
    assert_eq!(names(&mut dos, b"*.DAT", 0), ["NORMAL.DAT"]);
    assert_eq!(names(&mut dos, b"*.DAT", 2), ["HIDDEN.DAT", "NORMAL.DAT"]);
    assert_eq!(names(&mut dos, b"*.DAT", 4), ["NORMAL.DAT", "SYSTEM.DAT"]);
    assert_eq!(
        names(&mut dos, b"*.DAT", 6),
        ["BOTH.DAT", "HIDDEN.DAT", "NORMAL.DAT", "SYSTEM.DAT"]
    );
    success(&path_service(&mut dos, b"NORMAL.DAT", 0x4e00, 0));
    let data = dta(&mut dos, 0x10080);
    assert_eq!(&data[21..30], &[0x21, 0, 0, 0x21, 0, 3, 0, 0, 0]);
    assert_eq!(&data[30..], b"NORMAL.DAT\0\0\0");
    success(&path_service(&mut dos, b"BOTH.DAT", 0x4e00, 6));
    assert_eq!(dta(&mut dos, 0x10080)[21], 0x26);
    // Host replacement supplies a fresh ordinary file, including fresh attributes.
    dos.mount_file("BOTH.DAT", vec![], FileAccess::ReadWrite)
        .unwrap();
    assert_eq!(names(&mut dos, b"BOTH.DAT", 0), ["BOTH.DAT"]);
}

async fn check_discovery_program() {
    let mut dos = common::dos().await;
    for (name, contents) in [
        ("DATA/A.DAT", b"first\n".as_slice()),
        ("DATA/B.DAT", b"second\n"),
        ("DATA/OTHER.TXT", b"wrong extension"),
        ("DATA/SUB/C.DAT", b"wrong directory"),
    ] {
        dos.mount_file(name, contents.to_vec(), FileAccess::ReadOnly)
            .unwrap();
    }
    let mut program = vec![
        0xba, 0x80, 0x01, // MOV DX,0180: directory
        0xb4, 0x3b, 0xcd, 0x21, // Change directory.
        0x72, 0x4d, // JC failure
        0xba, 0x00, 0x03, // MOV DX,0300: DTA
        0xb4, 0x1a, 0xcd, 0x21, // Set DTA.
        0xba, 0x90, 0x01, // MOV DX,0190: pattern
        0x31, 0xc9, // XOR CX,CX: ordinary files
        0xb4, 0x4e, 0xcd, 0x21, // Find first.
        0x72, 0x3b, // JC failure
        0xba, 0x1e, 0x03, // loop: MOV DX,031E: name in DTA
        0xb8, 0x00, 0x3d, 0xcd, 0x21, // Open read-only.
        0x72, 0x31, // JC failure
        0x89, 0xc3, 0x89, 0xc6, // MOV BX,AX; MOV SI,AX: preserve handle
        0xb9, 0x20, 0x00, 0xba, 0x00, 0x04, // MOV CX,32; MOV DX,0400: buffer
        0xb4, 0x3f, 0xcd, 0x21, // Read file.
        0x72, 0x21, // JC failure
        0x89, 0xc1, 0xbb, 0x01, 0x00, // MOV CX,AX; MOV BX,1: stdout
        0xb4, 0x40, 0xcd, 0x21, // Write the bytes read.
        0x72, 0x16, // JC failure
        0x89, 0xf3, 0xb4, 0x3e, 0xcd, 0x21, // MOV BX,SI; close file.
        0xb4, 0x4f, 0xcd, 0x21, // Find next.
        0x73, 0xcf, // JNC loop
        0x83, 0xf8, 0x12, // CMP AX,18: no more files
        0x75, 0x05, // JNE failure
        0xb8, 0x00, 0x4c, 0xcd, 0x21, // Exit 0.
        0xb8, 0xff, 0x4c, 0xcd, 0x21, // failure: exit 255.
    ];
    program.resize(0x80, 0);
    program.extend_from_slice(b"DATA\0");
    program.resize(0x90, 0);
    program.extend_from_slice(b"*.DAT\0");
    dos.load_com(&program, Default::default()).unwrap();
    assert_eq!(dos.runtime().run(500).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(0)));
    assert_eq!(dos.take_output(), b"first\nsecond\n");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_discovers_opens_and_reads_matching_files() {
    pollster::block_on(check_discovery_program());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_discovers_opens_and_reads_matching_files() {
    check_discovery_program().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn searches_follow_dos_paths_and_continue_from_copied_dtas() {
    pollster::block_on(check_searches());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn searches_follow_dos_paths_and_continue_from_copied_dtas() {
    check_searches().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn searches_filter_attributes_and_encode_dos_find_data() {
    pollster::block_on(check_attributes());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn searches_filter_attributes_and_encode_dos_find_data() {
    check_attributes().await;
}
