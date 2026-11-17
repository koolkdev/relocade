use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Directory,
}

/// Metadata observed when listing an entry or querying an open file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub kind: EntryKind,
    /// File size in bytes; zero for directories, without counting their contents.
    pub len: u64,
    /// An absolute timestamp at the platform's precision. `None` means unavailable
    /// or outside this target's `SystemTime` range. Browsers do not expose directory
    /// modification times, and Wasm's `SystemTime` cannot represent dates before 1970.
    /// No timezone conversion or replacement with the current time occurs here.
    pub modified: Option<SystemTime>,
    /// Creation time, when supported by the filesystem. Browsers do not expose
    /// it. The same timestamp precision and representability rules apply.
    pub created: Option<SystemTime>,
}
