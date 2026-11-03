use super::*;
use futures_lite::future::{block_on, poll_once, zip};
use std::{
    io::ErrorKind,
    pin::pin,
    time::{Duration, UNIX_EPOCH},
};

#[path = "../../tests/common/mod.rs"]
mod common;

#[test]
fn native_worker_reads_file_contents_and_modification_time() {
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
        let files = FileService::new();
        let (name, backend) = platform::selected_file(path).await.unwrap();
        let file = files
            .resources
            .register_file(name, Access::ReadOnly, backend)
            .unwrap();
        let thread = web_thread::Thread::new();
        let task = thread.run_send((), move |()| common::read_contract(file, Some(modified)));
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
