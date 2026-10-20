use relocade_vfs::{Drives, MemoryFileSystem};
use std::io::ErrorKind;

#[test]
fn mounts_are_separate_and_drive_letters_are_case_insensitive() {
    let mut drives = Drives::new();
    assert!(drives.letters().next().is_none());
    let mut disk = MemoryFileSystem::new();
    disk.replace_file("FILE", b"D".to_vec()).unwrap();
    assert!(drives.mount('d', disk).unwrap().is_none());
    assert!(drives
        .mount('C', MemoryFileSystem::new())
        .unwrap()
        .is_none());
    drives
        .get_mut('c')
        .unwrap()
        .replace_file("FILE", b"C".to_vec())
        .unwrap();
    assert_eq!(drives.get('D').unwrap().read("FILE").unwrap(), b"D");
    assert_eq!(drives.get('C').unwrap().read("FILE").unwrap(), b"C");
    assert!(drives.get('E').is_none());
    assert_eq!(drives.letters().collect::<Vec<_>>(), ['C', 'D']);
    assert_eq!(drives.letters().next_back(), Some('D'));

    for letter in ['1', ':', 'é', '\0'] {
        assert_eq!(
            drives
                .mount(letter, MemoryFileSystem::new())
                .err()
                .unwrap()
                .kind(),
            ErrorKind::InvalidInput
        );
        assert!(drives.get(letter).is_none());
    }
    assert_eq!(drives.letters().collect::<Vec<_>>(), ['C', 'D']);
}

#[test]
fn replacing_a_mount_returns_the_old_filesystem_and_preserves_open_files() {
    let mut drives = Drives::new();
    let mut disk = MemoryFileSystem::new();
    let file = disk.replace_file("FILE", b"old".to_vec()).unwrap();
    drives.mount('C', disk).unwrap();
    let mut replacement = MemoryFileSystem::new();
    replacement.replace_file("FILE", b"new".to_vec()).unwrap();
    let previous = drives.mount('c', replacement).unwrap().unwrap();
    file.write_at(b"!", 0).unwrap();
    assert_eq!(previous.read("FILE").unwrap(), b"!ld");
    assert_eq!(drives.get('C').unwrap().read("FILE").unwrap(), b"new");
}
