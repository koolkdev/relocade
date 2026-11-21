#[path = "../../../crates/dos/tests/support/parallel.rs"]
mod fixture;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn parallel_files() -> Result<(), String> {
    let worker = relocade_machine::worker::Thread::new();
    worker
        .run(fixture::parallel_file_access)
        .await
        .map_err(|error| error.to_string())?
}
