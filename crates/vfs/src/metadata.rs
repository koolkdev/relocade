/// File or directory metadata at the time it was read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub(crate) directory: bool,
    pub(crate) len: u64,
    pub(crate) read_only: bool,
}

impl Metadata {
    pub fn is_dir(&self) -> bool {
        self.directory
    }

    pub fn is_file(&self) -> bool {
        !self.directory
    }

    /// File length in bytes; directories have length zero.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
}

/// An immediate child returned by [`crate::MemoryFileSystem::read_dir`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    pub(crate) name: String,
    pub(crate) metadata: Metadata,
}

impl DirEntry {
    /// The child's name, without its parent path.
    pub fn file_name(&self) -> &str {
        &self.name
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}
