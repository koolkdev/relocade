//! In-memory files and directories for guest operating-system environments.
//!
//! Paths are case-sensitive, root-relative UTF-8 strings separated by '/'.
//! Callers normalize guest paths; empty components, '.' and '..' are rejected.
//! The empty path names the root directory. File handles own their access rights
//! and share contents; guest layers maintain cursors and sharing rules.
//!
//! ```
//! use wasm86_vfs::MemoryFileSystem;
//!
//! let mut fs = MemoryFileSystem::new();
//! fs.create_dir_all("DATA")?;
//! let file = fs.create_new("DATA/SAVE.DAT")?;
//! file.write_at(b"saved", 0)?;
//!
//! let file = fs.open("DATA/SAVE.DAT")?;
//! let mut bytes = [0; 5];
//! file.read_at(&mut bytes, 0)?;
//! assert_eq!(&bytes, b"saved");
//!
//! for entry in fs.read_dir("DATA")? {
//!     assert_eq!(entry.file_name(), "SAVE.DAT");
//!     assert_eq!(entry.metadata().len(), 5);
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
#![forbid(unsafe_code)]

mod file;
mod filesystem;
mod metadata;

pub use file::File;
pub use filesystem::MemoryFileSystem;
pub use metadata::{DirEntry, Metadata};
