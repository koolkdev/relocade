mod common;

use relocade_dos::LoadError;
use relocade_x86::{ExecutionExit, Segment};

fn set_word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn executable() -> Vec<u8> {
    // Two header paragraphs, followed by code at 0001:0002 and data at 0004:0000.
    let mut file = vec![0; 0x20 + 0x44];
    file[..2].copy_from_slice(b"MZ");
    set_word(&mut file, 0x02, 0x64); // Bytes in the last file page.
    set_word(&mut file, 0x04, 1); // File pages.
    set_word(&mut file, 0x06, 1); // Relocation count.
    set_word(&mut file, 0x08, 2); // Header paragraphs.
    set_word(&mut file, 0x0a, 0x20); // Minimum extra paragraphs.
    set_word(&mut file, 0x0c, 0x30); // Maximum extra paragraphs.
    set_word(&mut file, 0x0e, 0x10); // Initial SS, relative to the load segment.
    set_word(&mut file, 0x10, 0x100); // Initial SP.
    set_word(&mut file, 0x14, 2); // Initial IP.
    set_word(&mut file, 0x16, 1); // Initial CS, relative to the load segment.
    set_word(&mut file, 0x18, 0x1c); // Relocation table begins after the header fields.
    set_word(&mut file, 0x1c, 3); // Relocate the MOV AX immediate at 0001:0003.
    set_word(&mut file, 0x1e, 1);
    let code = [
        0xb8, 4, 0, // MOV AX,0004 (relocated to the data segment)
        0x8e, 0xd8, // MOV DS,AX
        0xbb, 2, 0, // MOV BX,2
        0xb4, 0x48, 0xcd, 0x21, // Allocate two paragraphs.
        0x72, 0x18, // JC failure
        0x8e, 0xc0, // MOV ES,AX
        0xb4, 0x49, 0xcd, 0x21, // Free the allocated block.
        0xbb, 1, 0, // MOV BX,1 (stdout)
        0xb9, 4, 0, // MOV CX,4 (byte count)
        0xba, 0, 0, // MOV DX,0 (data offset)
        0xb4, 0x40, 0xcd, 0x21, // Write DS:DX.
        0xb8, 7, 0x4c, 0xcd, 0x21, // Exit 7.
        0xb8, 0xff, 0x4c, 0xcd, 0x21, // failure: exit 255.
    ];
    file[0x32..0x32 + code.len()].copy_from_slice(&code);
    file[0x60..0x64].copy_from_slice(b"MZ\r\n");
    file
}

async fn check_mz_execution() {
    let mut file = executable();
    file.extend_from_slice(b"not part of the load module");
    let mut dos = relocade_dos::Dos::new().unwrap();
    dos.launch_mz(&file, Default::default()).await.unwrap();
    {
        let mut machine = dos.cpu().unwrap().context();
        let cpu = machine.cpu().snapshot();
        assert_eq!(cpu.segments[Segment::Cs].selector, 0x1011);
        assert_eq!(cpu.eip, 2);
        assert_eq!(cpu.segments[Segment::Ss].selector, 0x1020);
        assert_eq!(cpu.registers.esp, 0x100);
        assert_eq!(cpu.segments[Segment::Ds].selector, 0x1000);
        assert_eq!(cpu.segments[Segment::Es].selector, 0x1000);
        assert_ne!(cpu.flags.eflags() & 0x200, 0);
        let mut psp = [0; 256];
        machine.memory().read(0x10000, &mut psp).unwrap();
        // PSP + five image paragraphs + 48 extra paragraphs.
        assert_eq!(&psp[..4], &[0xcd, 0x20, 0x45, 0x10]);
        assert_eq!(&psp[0x16..0x18], &[0, 0x10]);
        assert_eq!(&psp[0x80..0x82], &[0, 13]);
        let mut relocated = [0; 2];
        machine.memory().read(0x10113, &mut relocated).unwrap();
        assert_eq!(relocated, [0x14, 0x10]);
        let mut after_image = [0; 4];
        machine.memory().read(0x10144, &mut after_image).unwrap();
        assert_eq!(after_image, [0; 4]); // Ignore bytes beyond the declared file size.
    }
    assert_eq!(dos.cpu().unwrap().run(100).await, ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(7)));
    assert_eq!(common::output(dos.stdout()), b"MZ\r\n");

    // A requested maximum below the minimum uses the minimum; FFFF uses the arena.
    for (maximum, expected_end) in [(0x10, 0x1035_u16), (0xffff, 0x9ffe)] {
        let mut file = executable();
        set_word(&mut file, 0x0c, maximum);
        let mut dos = relocade_dos::Dos::new().unwrap();
        dos.launch_mz(&file, Default::default()).await.unwrap();
        let mut end = [0; 2];
        dos.cpu()
            .unwrap()
            .context()
            .memory()
            .read(0x10002, &mut end)
            .unwrap();
        assert_eq!(u16::from_le_bytes(end), expected_end);
        assert_eq!(dos.exit_status(), None);
    }

    // A zero final-page count means a complete 512-byte page.
    let mut file = executable();
    file.resize(512, 0);
    set_word(&mut file, 0x02, 0);
    let mut dos = relocade_dos::Dos::new().unwrap();
    dos.launch_mz(&file, Default::default()).await.unwrap();
    let mut end = [0; 2];
    dos.cpu()
        .unwrap()
        .context()
        .memory()
        .read(0x10002, &mut end)
        .unwrap();
    assert_eq!(u16::from_le_bytes(end), 0x105e);

    // MINALLOC=MAXALLOC=0 loads at the top of memory. Keep its stack within
    // the reserved bytes before the code; this program cannot allocate more.
    let mut file = executable();
    set_word(&mut file, 0x0a, 0);
    set_word(&mut file, 0x0c, 0);
    set_word(&mut file, 0x0e, 0);
    set_word(&mut file, 0x10, 0x12);
    let mut dos = relocade_dos::Dos::new().unwrap();
    dos.launch_mz(&file, Default::default()).await.unwrap();
    let cpu = dos.cpu().unwrap().context().cpu().snapshot();
    assert_eq!(cpu.segments[Segment::Cs].selector, 0x9ffa);
    assert_eq!(cpu.segments[Segment::Ss].selector, 0x9ff9);
    assert_eq!(cpu.segments[Segment::Ds].selector, 0x1000);
    assert_eq!(dos.cpu().unwrap().run(100).await, ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(255))); // AH=48h reports no space.
}

async fn check_invalid_mz() {
    let mut dos = common::dos(&[
        0xba, 0x0c, 1, 0xb4, 9, 0xcd, 0x21, // Print DS:010C.
        0xb8, 3, 0x4c, 0xcd, 0x21, // Exit 3.
        b'X', b'$',
    ])
    .await;
    assert_eq!(dos.cpu().unwrap().run(100).await, ExecutionExit::HostReturn);
    let cpu_before = dos.cpu().unwrap().context().cpu().snapshot();
    let memory = dos.memory();
    let mut before = vec![0; 0x90010];
    memory.view().read(0xfff0, &mut before).unwrap();
    let file = executable();
    // All short inputs and a truncated load module must be rejected.
    for length in 0..file.len() {
        assert!(
            dos.launch_mz(&file[..length], Default::default())
                .await
                .is_err(),
            "length {length}"
        );
    }
    for (offset, value, error) in [
        (0x00, 0, LoadError::InvalidMzHeader),      // Bad signature.
        (0x02, 512, LoadError::InvalidMzHeader),    // Invalid last-page length.
        (0x04, 0, LoadError::InvalidMzHeader),      // No file pages.
        (0x08, 1, LoadError::InvalidMzHeader),      // Header too small.
        (0x08, 7, LoadError::InvalidMzHeader),      // Header beyond file end.
        (0x06, 2, LoadError::InvalidRelocation),    // Table extends into load module.
        (0x18, 0x1b, LoadError::InvalidRelocation), // Table overlaps fixed fields.
        (0x1c, 0x33, LoadError::InvalidRelocation), // Relocated word straddles image end.
        (0x0a, 0xffff, LoadError::InsufficientMemory),
        (0x1a, 1, LoadError::UnsupportedOverlay),
    ] {
        let mut invalid = file.clone();
        set_word(&mut invalid, offset, value);
        assert_eq!(
            common::load_error(dos.launch_mz(&invalid, Default::default()).await),
            error,
            "header offset {offset:x}"
        );
    }
    let mut after = vec![0; before.len()];
    memory.view().read(0xfff0, &mut after).unwrap();
    assert_eq!(after, before);
    assert_eq!(dos.cpu().unwrap().context().cpu().snapshot(), cpu_before);
    assert_eq!(dos.exit_status(), Some(Ok(3)));
    assert_eq!(common::output(dos.stdout()), b"X");
}

#[test]
fn mz_relocates_allocates_prints_and_exits() {
    pollster::block_on(check_mz_execution());
}

#[test]
fn invalid_mz_preserves_the_terminated_program() {
    pollster::block_on(check_invalid_mz());
}
