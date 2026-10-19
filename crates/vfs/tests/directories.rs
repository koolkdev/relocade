use relocade_vfs::MemoryFileSystem;
use std::io::ErrorKind;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn listings_return_immediate_children_with_metadata() {
    let mut fs = MemoryFileSystem::new();
    fs.create_dir_all("DATA/EMPTY").unwrap();
    fs.create_dir_all("DATABASE").unwrap();
    fs.create_new("DATA/Z.DAT").unwrap();
    let file = fs.replace_file("DATA/A.DAT", b"abc".to_vec()).unwrap();
    file.set_read_only(true);
    fs.create_new("DATABASE/OTHER").unwrap();

    let root: Vec<_> = fs
        .read_dir("")
        .unwrap()
        .map(|entry| entry.file_name().to_owned())
        .collect();
    assert_eq!(root, ["DATA", "DATABASE"]);
    let entries: Vec<_> = fs.read_dir("DATA").unwrap().collect();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>(),
        ["A.DAT", "EMPTY", "Z.DAT"]
    );
    assert!(entries[0].metadata().is_file());
    assert!(!entries[0].metadata().is_dir());
    assert_eq!(entries[0].metadata().len(), 3);
    assert!(entries[0].metadata().is_read_only());
    assert!(entries[1].metadata().is_dir());
    assert!(!entries[1].metadata().is_file());
    assert_eq!(entries[1].metadata().len(), 0);
    assert!(!entries[2].metadata().is_read_only());
    assert!(fs.read_dir("DATA/EMPTY").unwrap().next().is_none());
    assert_eq!(fs.metadata("DATA/A.DAT").unwrap(), *entries[0].metadata());

    // Returned entries are snapshots; later metadata reads see current state.
    file.set_len(1).unwrap();
    file.set_read_only(false);
    assert_eq!(entries[0].metadata().len(), 3);
    assert!(entries[0].metadata().is_read_only());
    assert_eq!(fs.metadata("DATA/A.DAT").unwrap().len(), 1);
    assert!(!fs.metadata("DATA/A.DAT").unwrap().is_read_only());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn listing_uses_filesystem_path_errors_and_preserves_names() {
    let mut fs = MemoryFileSystem::new();
    assert!(fs.read_dir("").unwrap().next().is_none());
    fs.create_new("File").unwrap();
    fs.create_new("file").unwrap();
    fs.create_dir_all("café").unwrap();
    fs.create_new("café/雪").unwrap();
    assert_eq!(
        fs.read_dir("café").unwrap().next().unwrap().file_name(),
        "雪"
    );
    assert_eq!(
        fs.read_dir("")
            .unwrap()
            .map(|entry| entry.file_name().to_owned())
            .collect::<Vec<_>>(),
        ["File", "café", "file"]
    );
    for (path, expected) in [
        ("MISSING", ErrorKind::NotFound),
        ("File", ErrorKind::NotADirectory),
        ("File/CHILD", ErrorKind::NotADirectory),
        ("café/", ErrorKind::InvalidInput),
        ("café/../", ErrorKind::InvalidInput),
    ] {
        assert_eq!(fs.read_dir(path).err().unwrap().kind(), expected);
    }
}
