#![cfg(target_arch = "wasm32")]

use futures_lite::future::zip;
use relocade_file_picker::{Access, DirEntry, Directory, EntryKind, File, FileService, Metadata};
use std::{
    io::ErrorKind,
    time::{Duration, UNIX_EPOCH},
};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

mod common;
#[path = "common/directories.rs"]
mod directories;
#[path = "web/fixture.rs"]
mod fixture;
use fixture::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn rust_worker_reads_selected_file_and_metadata() {
    let files = FileService::new();
    complete_file_picker(false, 946_684_800_123.0);
    let file = files.pick_file().await.unwrap().unwrap();
    assert_eq!(file.name(), "PROGRAM.COM");
    let thread = web_thread::Thread::new();
    // Only the Rust proxy is captured; no browser file is posted to the worker.
    let task = thread.run_send((), move |()| {
        common::read_contract(
            file,
            Metadata {
                kind: EntryKind::File,
                len: 6,
                modified: Some(UNIX_EPOCH + Duration::from_millis(946_684_800_123)),
                created: None,
            },
        )
    });
    zip(
        async {
            task.await.unwrap().unwrap();
            complete_file_picker(true, 0.0);
            assert!(files.pick_file().await.unwrap().is_none());
            files.close();
        },
        files.run(),
    )
    .await;
}

#[wasm_bindgen_test]
async fn file_metadata_reports_unrepresentable_dates_as_missing() {
    let files = FileService::new();
    complete_file_picker(false, -1500.0);
    let file = files.pick_file().await.unwrap().unwrap();
    zip(
        async {
            assert_eq!(file.metadata().await.unwrap().modified, None);
            files.close();
        },
        files.run(),
    )
    .await;
}

#[wasm_bindgen_test]
async fn slow_browser_read_allows_other_resources_to_progress_and_shutdown_wakes_worker() {
    let files = FileService::new();
    complete_file_picker(false, 0.0);
    let file = files.pick_file().await.unwrap().unwrap();
    complete_file_picker(false, 0.0);
    let other = files.pick_file().await.unwrap().unwrap();
    let barrier = ReleaseReads(hold_next_read());
    let thread = web_thread::Thread::new();
    let task = thread.run_send((), move |()| async move {
        assert_eq!(file.read().await.unwrap_err().kind(), ErrorKind::BrokenPipe);
        assert_eq!(
            file.metadata().await.unwrap_err().kind(),
            ErrorKind::BrokenPipe
        );
    });
    zip(
        async {
            barrier.0.entered().await;
            assert_eq!(other.read().await.unwrap(), b"abcdef");
            files.close();
            task.await.unwrap();
            drop(barrier);
        },
        files.run(),
    )
    .await;
}

#[wasm_bindgen_test]
async fn directory_selection_preserves_access_and_reports_browser_outcomes() {
    let fixture = RestorePicker(mock_picker());
    let directory = directory_handle().await;
    let files = FileService::new();
    fixture.0.set("ok", &directory);
    for (access, mode) in [(Access::ReadOnly, "read"), (Access::ReadWrite, "readwrite")] {
        let selection = files.pick_directory(access).await.unwrap().unwrap();
        assert_eq!(fixture.0.mode(), mode);
        assert_eq!(
            fixture.0.receiver(),
            JsValue::from(web_sys::window().unwrap())
        );
        assert_eq!(selection.access(), access);
        assert!(!selection.name().is_empty());
    }
    fixture.0.set("missing", &directory);
    assert_eq!(
        files
            .pick_directory(Access::ReadOnly)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::Unsupported
    );
    fixture.0.set("AbortError", &directory);
    assert!(files
        .pick_directory(Access::ReadWrite)
        .await
        .unwrap()
        .is_none());
    for failure in ["NotAllowedError", "throw"] {
        fixture.0.set(failure, &directory);
        assert_eq!(
            files
                .pick_directory(Access::ReadWrite)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::PermissionDenied
        );
        assert_eq!(fixture.0.mode(), "readwrite");
    }
    fixture.0.set("NotReadableError", &directory);
    assert!(files
        .pick_directory(Access::ReadOnly)
        .await
        .unwrap_err()
        .to_string()
        .contains("NotReadableError"));
    files.close();
    remove_directory(&directory).await;
}

#[wasm_bindgen_test]
async fn rust_worker_browses_selected_directory() {
    let fixture = RestorePicker(mock_picker());
    let directory = directory_handle().await;
    let modified = UNIX_EPOCH + Duration::from_millis(seed_directory(&directory).await as u64);
    fixture.0.set("ok", &directory);
    let files = FileService::new();
    let root = files
        .pick_directory(Access::ReadOnly)
        .await
        .unwrap()
        .unwrap();
    let thread = web_thread::Thread::new();
    let task = thread.run_send((), move |()| {
        directories::browse_contract(
            root,
            Metadata {
                kind: EntryKind::Directory,
                len: 0,
                modified: None,
                created: None,
            },
            Metadata {
                kind: EntryKind::File,
                len: 6,
                modified: Some(modified),
                created: None,
            },
        )
    });
    zip(
        async {
            task.await.unwrap().unwrap();
            files.close();
        },
        files.run(),
    )
    .await;
    remove_directory(&directory).await;
}
