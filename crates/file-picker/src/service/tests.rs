use super::*;
use crate::{DirEntry, Directory, EntryKind, Metadata};
use futures_lite::future::{block_on, poll_once, zip};
use std::{
    io::ErrorKind,
    pin::pin,
    time::{Duration, UNIX_EPOCH},
};

#[path = "../../tests/common/mod.rs"]
mod common;
#[path = "../../tests/common/directories.rs"]
mod directories;

#[test]
fn native_worker_reads_file_contents_and_metadata() {
    block_on(async {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("PROGRAM.COM");
        std::fs::write(&path, b"abcdef").unwrap();
        let modified = UNIX_EPOCH + Duration::from_millis(946_684_800_123);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let metadata = Metadata {
            kind: EntryKind::File,
            len: 6,
            modified: Some(modified),
            created: std::fs::metadata(&path).unwrap().created().ok(),
        };
        let files = FileService::new();
        let (name, backend) = platform::selected_file(path).await.unwrap();
        let file = files
            .resources
            .register_file(name, Access::ReadOnly, backend)
            .unwrap();
        let thread = web_thread::Thread::new();
        let task = thread.run_send((), move |()| common::read_contract(file, metadata));
        zip(
            async {
                task.await.unwrap().unwrap();
                files.close();
            },
            files.run(),
        )
        .await;
    });
}

#[test]
fn closing_before_run_rejects_queued_requests_and_surviving_handles() {
    block_on(async {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let files = FileService::new();
        let (name, backend) = platform::selected_file(temporary.path().into())
            .await
            .unwrap();
        let file = files
            .resources
            .register_file(name, Access::ReadOnly, backend)
            .unwrap();
        let mut pending = pin!(file.read());
        assert!(poll_once(&mut pending).await.is_none());
        let mut waiting = pin!(file.metadata());
        assert!(poll_once(&mut waiting).await.is_none());
        files.close();
        assert_eq!(pending.await.unwrap_err().kind(), ErrorKind::BrokenPipe);
        assert_eq!(waiting.await.unwrap_err().kind(), ErrorKind::BrokenPipe);
        assert_eq!(
            file.metadata().await.unwrap_err().kind(),
            ErrorKind::BrokenPipe
        );
        files.run().await;
    });
}

#[cfg(unix)]
#[test]
fn file_selection_rejects_fifos_without_waiting_for_a_writer() {
    block_on(async {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("PIPE");
        rustix::fs::mkfifoat(rustix::fs::CWD, &path, rustix::fs::Mode::RUSR).unwrap();
        assert_eq!(
            platform::selected_file(path).await.err().unwrap().kind(),
            ErrorKind::InvalidInput
        );
    });
}

#[test]
fn native_worker_browses_selected_directory_with_inherited_access() {
    block_on(async {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::create_dir(temporary.path().join("DATA")).unwrap();
        std::fs::write(temporary.path().join("DATA/GAME.EXE"), b"abcdef").unwrap();
        let modified = UNIX_EPOCH + Duration::from_millis(946_684_800_123);
        std::fs::File::options()
            .write(true)
            .open(temporary.path().join("DATA/GAME.EXE"))
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let file_metadata = Metadata {
            kind: EntryKind::File,
            len: 6,
            modified: Some(modified),
            created: std::fs::metadata(temporary.path().join("DATA/GAME.EXE"))
                .unwrap()
                .created()
                .ok(),
        };
        let stat = std::fs::metadata(temporary.path().join("DATA")).unwrap();
        let directory_metadata = Metadata {
            kind: EntryKind::Directory,
            len: 0,
            modified: stat.modified().ok(),
            created: stat.created().ok(),
        };
        let files = FileService::new();
        let (name, backend) = platform::selected_directory(temporary.path().into())
            .await
            .unwrap();
        let root = files
            .resources
            .register_directory(name, Access::ReadOnly, backend)
            .unwrap();
        let thread = web_thread::Thread::new();
        let task = thread.run_send((), move |()| {
            directories::browse_contract(root, directory_metadata, file_metadata)
        });
        zip(
            async {
                task.await.unwrap().unwrap();
                files.close();
            },
            files.run(),
        )
        .await;
    });
}

#[cfg(unix)]
#[test]
fn rooted_directory_rejects_escape_links_and_special_files() {
    block_on(async {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::create_dir(temporary.path().join("root")).unwrap();
        std::fs::write(temporary.path().join("outside"), b"private").unwrap();
        std::os::unix::fs::symlink("../outside", temporary.path().join("root/link")).unwrap();
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            temporary.path().join("root/PIPE"),
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        let files = FileService::new();
        let (name, backend) = platform::selected_directory(temporary.path().join("root"))
            .await
            .unwrap();
        let root = files
            .resources
            .register_directory(name, Access::ReadWrite, backend)
            .unwrap();
        zip(
            async {
                assert!(root.read_dir().await.unwrap().is_empty());
                for access in [Access::ReadOnly, Access::ReadWrite] {
                    assert!(root.open_file("link", access).await.is_err());
                    assert_eq!(
                        root.open_file("PIPE", access).await.unwrap_err().kind(),
                        ErrorKind::InvalidInput
                    );
                }
                files.close();
            },
            files.run(),
        )
        .await;
        assert_eq!(
            std::fs::read(temporary.path().join("outside")).unwrap(),
            b"private"
        );
    });
}
