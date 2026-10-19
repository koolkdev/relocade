use relocade_vfs::MemoryFileSystem;
use std::io::ErrorKind;

#[test]
fn directories_exist_independently_and_exclude_files() {
    let mut fs = MemoryFileSystem::new();
    assert!(fs.is_dir(""));
    assert_eq!(fs.open("").unwrap_err().kind(), ErrorKind::IsADirectory);
    assert_eq!(
        fs.create_new("DATA/SAVE/FILE").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    fs.create_dir_all("DATA/SAVE").unwrap();
    fs.create_dir_all("DATA/SAVE").unwrap();
    assert!(fs.is_dir("DATA/SAVE")); // An empty directory is an entry of its own.
    assert!(!fs.is_dir("data/save"));
    let file = fs.create_new("DATA/SAVE/FILE").unwrap();
    file.write_at(b"keep", 0).unwrap();
    assert_eq!(
        fs.create_new("DATA").unwrap_err().kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(
        fs.create_dir_all("DATA/SAVE/FILE/CHILD")
            .unwrap_err()
            .kind(),
        ErrorKind::NotADirectory
    );
    assert_eq!(
        fs.open("DATA/SAVE/FILE/MISSING/CHILD").unwrap_err().kind(),
        ErrorKind::NotADirectory
    );
    assert_eq!(
        fs.create_new("DATA/SAVE/FILE/MISSING/CHILD")
            .unwrap_err()
            .kind(),
        ErrorKind::NotADirectory
    );
    assert_eq!(
        fs.replace_file("DATA/SAVE/FILE/MISSING/CHILD", Vec::new())
            .unwrap_err()
            .kind(),
        ErrorKind::NotADirectory
    );
    assert_eq!(
        fs.replace_file("DATA/SAVE", Vec::new()).unwrap_err().kind(),
        ErrorKind::IsADirectory
    );
    assert_eq!(
        fs.open("data/SAVE/FILE").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    for path in [
        "/DATA",
        "DATA/",
        "DATA//X",
        "DATA/./X",
        "DATA/../X",
        "DATA/\0X",
    ] {
        assert_eq!(
            fs.create_new(path).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(fs.read("DATA/SAVE/FILE").unwrap(), b"keep");
}

#[test]
fn handles_preserve_identity_and_access_across_changes() {
    let mut fs = MemoryFileSystem::new();
    let writer = fs.create_new("SAVE").unwrap();
    writer.write_at(b"old", 0).unwrap();
    assert_eq!(
        fs.create_new("SAVE").unwrap_err().kind(),
        ErrorKind::AlreadyExists
    );
    let reader = fs.open("SAVE").unwrap();
    assert!(writer.same_file(&reader));
    assert!(writer.same_file(&writer.clone()));
    assert!(writer.same_file(&fs.open_read_write("SAVE").unwrap()));
    assert_eq!(
        reader.write_at(b"!", 0).unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        reader.set_len(0).unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(fs.read("SAVE").unwrap(), b"old");

    writer.set_read_only(true);
    assert!(reader.is_read_only());
    assert_eq!(
        fs.open_read_write("SAVE").unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    writer.write_at(b"new", 0).unwrap(); // Existing handles retain their write access.
    assert_eq!(fs.read("SAVE").unwrap(), b"new");

    let replacement = fs.replace_file("SAVE", b"replacement".to_vec()).unwrap();
    assert!(!writer.same_file(&replacement));
    assert!(!replacement.is_read_only());
    assert_eq!(fs.read("SAVE").unwrap(), b"replacement");
    writer.set_len(1).unwrap();
    assert_eq!(reader.len(), 1);
    let mut bytes = [0; 3];
    assert_eq!(reader.read_at(&mut bytes, 0).unwrap(), 1);
    assert_eq!(bytes, [b'n', 0, 0]);
    writer.set_len(0).unwrap();
    assert!(reader.is_empty());
    assert_eq!(fs.read("SAVE").unwrap(), b"replacement");
}

#[test]
fn removing_a_file_preserves_open_handles_and_allows_reusing_its_name() {
    let mut fs = MemoryFileSystem::new();
    fs.create_dir_all("DATA").unwrap();
    let writer = fs.replace_file("DATA/FILE", b"old".to_vec()).unwrap();
    let reader = fs.open("DATA/FILE").unwrap();
    writer.set_read_only(true);
    fs.remove_file("DATA/FILE").unwrap();
    assert_eq!(
        fs.open("DATA/FILE").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    assert!(fs.read_dir("DATA").unwrap().next().is_none());

    let replacement = fs.replace_file("DATA/FILE", b"new".to_vec()).unwrap();
    assert!(!writer.same_file(&replacement));
    writer.write_at(b"!", 0).unwrap();
    let mut bytes = [0; 3];
    assert_eq!(reader.read_at(&mut bytes, 0).unwrap(), 3);
    assert_eq!(&bytes, b"!ld");
    assert_eq!(fs.read("DATA/FILE").unwrap(), b"new");

    for (path, expected) in [
        ("", ErrorKind::IsADirectory),
        ("DATA", ErrorKind::IsADirectory),
        ("MISSING", ErrorKind::NotFound),
        ("MISSING/FILE", ErrorKind::NotFound),
        ("DATA/FILE/CHILD", ErrorKind::NotADirectory),
        ("DATA/../FILE", ErrorKind::InvalidInput),
    ] {
        assert_eq!(fs.remove_file(path).unwrap_err().kind(), expected);
    }
    assert_eq!(fs.read("DATA/FILE").unwrap(), b"new");
}

#[test]
fn positioned_io_zero_fills_and_checks_64_bit_ranges() {
    let mut fs = MemoryFileSystem::new();
    let file = fs.create_new("DATA").unwrap();
    assert_eq!(file.write_at(b"ab", 2).unwrap(), 2);
    file.set_len(6).unwrap();
    assert_eq!(fs.read("DATA").unwrap(), b"\0\0ab\0\0");
    file.set_len(3).unwrap();
    assert_eq!(fs.read("DATA").unwrap(), b"\0\0a");
    assert_eq!(file.len(), 3u64);
    assert_eq!(file.write_at(b"", u64::MAX).unwrap(), 0);

    let mut buffer = [0xcc; 2];
    assert_eq!(
        file.read_at(&mut buffer, u64::from(u32::MAX) + 1).unwrap(),
        0
    );
    assert_eq!(file.read_at(&mut buffer, u64::MAX).unwrap(), 0);
    assert_eq!(buffer, [0xcc; 2]);
    assert_eq!(
        file.write_at(b"x", u64::MAX).unwrap_err().kind(),
        ErrorKind::FileTooLarge
    );
    assert_eq!(
        file.set_len(u64::MAX).unwrap_err().kind(),
        ErrorKind::FileTooLarge
    );
    #[cfg(target_pointer_width = "32")]
    assert_eq!(
        file.write_at(b"x", 1u64 << 32).unwrap_err().kind(),
        ErrorKind::FileTooLarge
    );
    assert_eq!(fs.read("DATA").unwrap(), b"\0\0a");
}
