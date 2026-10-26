use super::{FileAccess, FileSystem, Files, Handle};

#[test]
fn retained_file_handle_keeps_its_cursor_after_handle_replacement_and_reuse() {
    let mut filesystem = FileSystem::default();
    filesystem
        .mount_file("FIRST.TXT", b"ABC".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    filesystem
        .mount_file("SECOND.TXT", b"xyz".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    let mut files = Files::default();
    let first = files.open(&filesystem, b"FIRST.TXT", 0x42).unwrap();
    let Handle::File(retained) = files.handle(first).unwrap().clone() else {
        panic!("opened a file");
    };
    let mut byte = [0];
    assert_eq!(retained.read(&mut byte), Ok(1));
    assert_eq!(byte, *b"A");

    let second = files.open(&filesystem, b"SECOND.TXT", 0x42).unwrap();
    files
        .force_duplicate(&mut filesystem, second, first)
        .unwrap();
    files.close(&mut filesystem, second).unwrap();
    assert_eq!(files.read(first, &mut byte), Ok(1));
    assert_eq!(byte, *b"x");
    assert_eq!(retained.read(&mut byte), Ok(1));
    assert_eq!(byte, *b"B");
    files.close(&mut filesystem, first).unwrap();

    let reopened = files.open(&filesystem, b"FIRST.TXT", 0x42).unwrap();
    assert_eq!(reopened, first); // Exercise reuse of the same DOS handle slot.
    assert_eq!(files.read(reopened, &mut byte), Ok(1));
    assert_eq!(byte, *b"A");
    assert_eq!(retained.read(&mut byte), Ok(1));
    assert_eq!(byte, *b"C"); // The retained record kept its original cursor.
}
