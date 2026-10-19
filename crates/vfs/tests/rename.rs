use relocade_vfs::MemoryFileSystem;
use std::io::ErrorKind;

#[test]
fn rename_moves_file_identity_and_preserves_handles_to_a_replaced_file() {
    let mut fs = MemoryFileSystem::new();
    fs.create_dir("DATA").unwrap();
    let source = fs.replace_file("OLD", b"source".to_vec()).unwrap();
    let replaced = fs
        .replace_file("DATA/NEW", b"destination".to_vec())
        .unwrap();
    source.set_read_only(true);
    replaced.set_read_only(true);

    fs.rename_file("OLD", "DATA/NEW").unwrap();
    assert_eq!(fs.open("OLD").unwrap_err().kind(), ErrorKind::NotFound);
    let renamed = fs.open("DATA/NEW").unwrap();
    assert!(renamed.same_file(&source));
    assert!(renamed.is_read_only());
    assert_eq!(fs.read("DATA/NEW").unwrap(), b"source");
    let mut bytes = [0; 11];
    assert_eq!(replaced.read_at(&mut bytes, 0).unwrap(), 11);
    assert_eq!(&bytes, b"destination");

    source.write_at(b"!", 0).unwrap();
    assert_eq!(fs.read("DATA/NEW").unwrap(), b"!ource");
    replaced.write_at(b"?", 0).unwrap();
    assert_eq!(replaced.read_at(&mut bytes, 0).unwrap(), 11);
    assert_eq!(&bytes, b"?estination");
    assert_eq!(fs.read("DATA/NEW").unwrap(), b"!ource");
    fs.rename_file("DATA/NEW", "DATA/NEW").unwrap();
    assert!(fs.open("DATA/NEW").unwrap().same_file(&source));

    fs.rename_file("DATA/NEW", "MOVED").unwrap();
    assert!(fs.read_dir("DATA").unwrap().next().is_none());
    assert!(fs.open("MOVED").unwrap().same_file(&source));
}

#[test]
fn invalid_renames_leave_source_and_destination_unchanged() {
    let mut fs = MemoryFileSystem::new();
    fs.create_dir("DIR").unwrap();
    fs.replace_file("SOURCE", b"source".to_vec()).unwrap();
    fs.replace_file("DEST", b"destination".to_vec()).unwrap();
    for (from, to, kind) in [
        ("MISSING", "DEST", ErrorKind::NotFound),
        ("MISSING/FILE", "DEST", ErrorKind::NotFound),
        ("SOURCE/FILE", "DEST", ErrorKind::NotADirectory),
        ("DIR/../SOURCE", "DEST", ErrorKind::InvalidInput),
        ("DIR", "DEST", ErrorKind::IsADirectory),
        ("", "DEST", ErrorKind::IsADirectory),
        ("SOURCE", "DIR", ErrorKind::IsADirectory),
        ("SOURCE", "", ErrorKind::IsADirectory),
        ("SOURCE", "MISSING/FILE", ErrorKind::NotFound),
        ("SOURCE", "DEST/FILE", ErrorKind::NotADirectory),
        ("SOURCE", "DIR/../DEST", ErrorKind::InvalidInput),
    ] {
        assert_eq!(fs.rename_file(from, to).unwrap_err().kind(), kind);
        assert_eq!(fs.read("SOURCE").unwrap(), b"source");
        assert_eq!(fs.read("DEST").unwrap(), b"destination");
        assert!(fs.is_dir("DIR"));
    }
}
