use relocade_vfs::{MemoryFileSystem, Space};
use std::io::ErrorKind;

#[test]
fn volume_geometry_is_configurable_without_allocating_file_data() {
    assert_eq!(
        MemoryFileSystem::new().space(),
        Space {
            allocation_unit: 16_384,
            total_units: 32_768,
            free_units: 32_768,
        }
    );
    for (capacity, unit) in [(8, 0), (9, 4)] {
        assert_eq!(
            MemoryFileSystem::with_capacity(capacity, unit)
                .err()
                .unwrap()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    let mut fs = MemoryFileSystem::with_capacity(0, 4).unwrap();
    fs.create_dir("DIR").unwrap();
    let file = fs.create_new("DIR/EMPTY").unwrap();
    assert_eq!(
        file.write_at(b"x", 0).unwrap_err().kind(),
        ErrorKind::StorageFull
    );
    assert!(file.is_empty());
    assert_eq!(fs.space().free_units, 0);
}

#[test]
fn file_growth_rounds_allocation_and_returns_short_writes_at_capacity() {
    let mut fs = MemoryFileSystem::with_capacity(12, 4).unwrap();
    let first = fs.create_new("FIRST").unwrap();
    let second = fs.create_new("SECOND").unwrap();
    first.write_at(b"a", 0).unwrap();
    assert_eq!(fs.space().free_units, 2);
    second.write_at(b"b", 0).unwrap();
    assert_eq!(fs.space().free_units, 1);
    let alias = first.clone();
    let reader = fs.open("FIRST").unwrap();
    assert_eq!(alias.write_at(b"123456789", 1).unwrap(), 7);
    assert_eq!(fs.space().free_units, 0);
    assert_eq!(fs.read("FIRST").unwrap(), b"a1234567");
    assert_eq!(second.write_at(b"cdef", 1).unwrap(), 3); // Space inside an allocated unit.
    assert_eq!(fs.read("SECOND").unwrap(), b"bcde");
    assert_eq!(
        first.write_at(b"x", 8).unwrap_err().kind(),
        ErrorKind::StorageFull
    );
    assert_eq!(first.write_at(b"", u64::MAX).unwrap(), 0);
    assert_eq!(first.set_len(9).unwrap_err().kind(), ErrorKind::StorageFull);
    assert_eq!(
        reader.set_len(0).unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        first.set_len(u64::MAX).unwrap_err().kind(),
        ErrorKind::FileTooLarge
    );
    assert_eq!(fs.read("FIRST").unwrap(), b"a1234567");
    assert_eq!(fs.space().free_units, 0);
    first.set_len(4).unwrap();
    assert_eq!(fs.space().free_units, 1);
    first.set_len(3).unwrap();
    assert_eq!(fs.space().free_units, 1);
    assert_eq!(alias.write_at(b"xyz", 6).unwrap(), 2);
    assert_eq!(fs.read("FIRST").unwrap(), b"a12\0\0\0xy");
    assert_eq!(fs.space().free_units, 0);
    first.set_len(0).unwrap();
    assert_eq!(fs.space().free_units, 2);
}

#[test]
fn unlinked_files_release_allocation_only_after_the_last_handle() {
    let mut fs = MemoryFileSystem::with_capacity(12, 4).unwrap();
    let file = fs.replace_file("FILE", vec![1; 5]).unwrap();
    let alias = fs.open("FILE").unwrap();
    fs.remove_file("FILE").unwrap();
    assert_eq!(fs.space().free_units, 1);
    fs.replace_file("FILE", vec![2; 1]).unwrap();
    assert_eq!(fs.space().free_units, 0);
    drop(file);
    assert_eq!(fs.space().free_units, 0);
    drop(alias);
    assert_eq!(fs.space().free_units, 2);
    fs.remove_file("FILE").unwrap();
    assert_eq!(fs.space().free_units, 3);
}

#[test]
fn replacement_reuses_unheld_allocation_and_failure_preserves_the_old_file() {
    let mut fs = MemoryFileSystem::with_capacity(8, 4).unwrap();
    fs.replace_file("FILE", vec![1; 5]).unwrap();
    fs.replace_file("FILE", vec![2; 8]).unwrap();
    assert_eq!(fs.space().free_units, 0);
    assert_eq!(
        fs.replace_file("FILE", vec![3; 9]).unwrap_err().kind(),
        ErrorKind::StorageFull
    );
    assert_eq!(fs.read("FILE").unwrap(), vec![2; 8]);
    assert_eq!(fs.space().free_units, 0);
    let old = fs.open_read_write("FILE").unwrap();
    assert_eq!(
        fs.replace_file("FILE", vec![3; 1]).unwrap_err().kind(),
        ErrorKind::StorageFull
    );
    old.set_len(4).unwrap();
    let replacement = fs.replace_file("FILE", vec![3; 1]).unwrap();
    assert!(!old.same_file(&replacement));
    assert_eq!(fs.space().free_units, 0);
    assert_eq!(old.set_len(5).unwrap_err().kind(), ErrorKind::StorageFull);
    drop(old);
    assert_eq!(fs.space().free_units, 1);
    assert_eq!(fs.read("FILE").unwrap(), vec![3; 1]);
}

#[test]
fn rename_and_remount_keep_allocations_with_their_original_volumes() {
    let mut fs = MemoryFileSystem::with_capacity(12, 4).unwrap();
    fs.replace_file("FROM", vec![1; 4]).unwrap();
    let overwritten = fs.replace_file("TO", vec![2; 8]).unwrap();
    fs.rename_file("FROM", "TO").unwrap();
    assert_eq!(fs.space().free_units, 0);
    drop(overwritten);
    assert_eq!(fs.space().free_units, 2);
    let old = fs.open_read_write("TO").unwrap();
    let mut drives = relocade_vfs::Drives::new();
    drives.mount('C', fs).unwrap();
    drives
        .mount('C', MemoryFileSystem::with_capacity(8, 4).unwrap())
        .unwrap();
    old.set_len(12).unwrap();
    assert_eq!(old.set_len(13).unwrap_err().kind(), ErrorKind::StorageFull);
    assert_eq!(drives.get('C').unwrap().space().free_units, 2);
}
