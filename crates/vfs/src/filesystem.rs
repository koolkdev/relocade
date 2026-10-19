use crate::File;
use std::collections::{btree_map::Entry, BTreeMap};
use std::io::{self, ErrorKind};

/// A filesystem with explicit directories and shared in-memory files.
pub struct MemoryFileSystem {
    entries: BTreeMap<String, Node>,
}

enum Node {
    Directory,
    File(File),
}

impl Default for MemoryFileSystem {
    fn default() -> Self {
        Self {
            entries: BTreeMap::from([(String::new(), Node::Directory)]),
        }
    }
}

impl MemoryFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens an existing file for reading.
    pub fn open(&self, path: &str) -> io::Result<File> {
        self.open_file(path, false)
    }

    /// Opens an existing file for reading and writing without truncating it.
    pub fn open_read_write(&self, path: &str) -> io::Result<File> {
        self.open_file(path, true)
    }

    fn open_file(&self, path: &str, writable: bool) -> io::Result<File> {
        validate_path(path)?;
        self.require_parents(path)?;
        match self.entries.get(path) {
            Some(Node::File(file)) => file.open(writable),
            Some(Node::Directory) => Err(ErrorKind::IsADirectory.into()),
            None => Err(ErrorKind::NotFound.into()),
        }
    }

    /// Creates an empty file for reading and writing. Fails if the path exists.
    pub fn create_new(&mut self, path: &str) -> io::Result<File> {
        validate_path(path)?;
        self.require_parents(path)?;
        match self.entries.entry(path.to_owned()) {
            Entry::Occupied(_) => Err(ErrorKind::AlreadyExists.into()),
            Entry::Vacant(entry) => {
                let file = File::new(Vec::new());
                entry.insert(Node::File(file.clone()));
                Ok(file)
            }
        }
    }

    /// Creates a directory and any missing parents; existing directories succeed.
    pub fn create_dir_all(&mut self, path: &str) -> io::Result<()> {
        validate_path(path)?;
        if path.is_empty() {
            return Ok(());
        }
        let mut directory = String::new();
        for component in path.split('/') {
            if !directory.is_empty() {
                directory.push('/');
            }
            directory.push_str(component);
            match self.entries.entry(directory.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(Node::Directory);
                }
                Entry::Occupied(entry) if matches!(entry.get(), Node::Directory) => {}
                Entry::Occupied(_) => return Err(ErrorKind::NotADirectory.into()),
            }
        }
        Ok(())
    }

    pub fn is_dir(&self, path: &str) -> bool {
        matches!(self.entries.get(path), Some(Node::Directory))
    }

    /// Reads a file's complete contents.
    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        Ok(self.open(path)?.read_all())
    }

    /// Supplies host-owned contents as a new file, replacing any file at the path.
    /// Existing handles keep the old file. Its read-only flag does not prevent
    /// replacement; the new file starts writable. Parent directories must exist.
    pub fn replace_file(&mut self, path: &str, contents: Vec<u8>) -> io::Result<File> {
        validate_path(path)?;
        self.require_parents(path)?;
        if self.is_dir(path) {
            return Err(ErrorKind::IsADirectory.into());
        }
        let file = File::new(contents);
        self.entries
            .insert(path.to_owned(), Node::File(file.clone()));
        Ok(file)
    }

    fn require_parents(&self, path: &str) -> io::Result<()> {
        for (separator, _) in path.match_indices('/') {
            match self.entries.get(&path[..separator]) {
                Some(Node::Directory) => {}
                Some(Node::File(_)) => return Err(ErrorKind::NotADirectory.into()),
                None => return Err(ErrorKind::NotFound.into()),
            }
        }
        Ok(())
    }
}

fn validate_path(path: &str) -> io::Result<()> {
    if !path.is_empty()
        && path
            .split('/')
            .any(|part| matches!(part, "" | "." | "..") || part.contains('\0'))
    {
        return Err(ErrorKind::InvalidInput.into());
    }
    Ok(())
}
