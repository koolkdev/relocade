#![cfg(target_arch = "wasm32")]

use futures_lite::future::zip;
use relocade_file_picker::{Access, File, FileService};
use std::{
    io::ErrorKind,
    time::{Duration, UNIX_EPOCH},
};
use wasm_bindgen_test::*;

mod common;
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
            Some(UNIX_EPOCH + Duration::from_millis(946_684_800_123)),
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
