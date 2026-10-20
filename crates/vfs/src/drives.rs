use crate::MemoryFileSystem;
use std::collections::BTreeMap;
use std::io::{self, ErrorKind};

/// Filesystems mounted at case-insensitive drive letters A through Z.
/// Guest environments own path parsing and current-directory state.
#[derive(Default)]
pub struct Drives {
    filesystems: BTreeMap<char, MemoryFileSystem>,
}

impl Drives {
    /// Creates an empty drive table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mounts a filesystem, returning the previous mount if the letter was in use.
    /// Invalid letters return InvalidInput without changing any mount.
    /// Existing file handles retain their contents and access rights.
    pub fn mount(
        &mut self,
        letter: char,
        filesystem: MemoryFileSystem,
    ) -> io::Result<Option<MemoryFileSystem>> {
        if !letter.is_ascii_alphabetic() {
            return Err(ErrorKind::InvalidInput.into());
        }
        Ok(self
            .filesystems
            .insert(letter.to_ascii_uppercase(), filesystem))
    }

    pub fn get(&self, letter: char) -> Option<&MemoryFileSystem> {
        self.filesystems.get(&letter.to_ascii_uppercase())
    }

    pub fn get_mut(&mut self, letter: char) -> Option<&mut MemoryFileSystem> {
        self.filesystems.get_mut(&letter.to_ascii_uppercase())
    }

    /// Mounted letters in ascending uppercase order.
    pub fn letters(&self) -> impl DoubleEndedIterator<Item = char> + '_ {
        self.filesystems.keys().copied()
    }
}
