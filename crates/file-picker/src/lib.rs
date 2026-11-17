//! Main-thread file selection and I/O, with Rust-only handles for other workers.
//!
//! Poll [`FileService::run`] alongside the application's main-thread lifecycle.
//! File selection runs there too. [`File`] is `Clone + Send + Sync` and uses Rust
//! channels. It contains no browser object or platform path.
//!
//! Native disk operations use a blocking I/O pool. Browser promises stay on the
//! service's thread. The caller owns application scheduling and DOS/VFS mounting.
//!
//! ```no_run
//! use relocade_file_picker::FileService;
//! # async fn example() -> std::io::Result<()> {
//! let files = FileService::new();
//! let work = async {
//!     if let Some(file) = files.pick_file().await? {
//!         // This handle may also be sent to the existing execution worker.
//!         let metadata = file.metadata().await?;
//!         let mut header = [0; 32];
//!         let count = file.read_at(&mut header, 0).await?;
//!     }
//!     Ok::<_, std::io::Error>(())
//! };
//! let (result, ()) = futures_lite::future::zip(async {
//!     let result = work.await;
//!     files.close();
//!     result
//! }, files.run()).await;
//! result
//! # }
//! ```
#![forbid(unsafe_code)]

mod file;
mod service;
pub use file::{File, FileMetadata};
pub use service::FileService;

#[cfg(not(target_arch = "wasm32"))]
#[path = "platform/native.rs"]
mod platform;
#[cfg(target_arch = "wasm32")]
#[path = "platform/web.rs"]
mod platform;

/// Access enforced by the service for a directory or file.
/// Read/write access permits requests; OS or browser permissions can still deny them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}
