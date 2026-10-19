use crate::{DirEntry, File, Metadata};
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

impl Node {
    fn metadata(&self) -> Metadata {
        match self {
            Self::Directory => Metadata {
                directory: true,
                len: 0,
                read_only: false,
            },
            Self::File(file) => Metadata {
                directory: false,
                len: file.len(),
                read_only: file.is_read_only(),
            },
        }
    }
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

    /// Creates one directory. Its parents must exist and its path must not exist.
    pub fn create_dir(&mut self, path: &str) -> io::Result<()> {
        validate_path(path)?;
        self.require_parents(path)?;
        match self.entries.entry(path.to_owned()) {
            Entry::Occupied(_) => Err(ErrorKind::AlreadyExists.into()),
            Entry::Vacant(entry) => {
                entry.insert(Node::Directory);
                Ok(())
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
            match self.create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if !self.is_dir(&directory) {
                        return Err(ErrorKind::NotADirectory.into());
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn is_dir(&self, path: &str) -> bool {
        matches!(self.entries.get(path), Some(Node::Directory))
    }

    pub fn metadata(&self, path: &str) -> io::Result<Metadata> {
        validate_path(path)?;
        self.require_parents(path)?;
        self.entries
            .get(path)
            .map(Node::metadata)
            .ok_or_else(|| ErrorKind::NotFound.into())
    }

    /// Lists immediate children in filename order, without '.' or '..'.
    /// Each yielded entry owns a snapshot of its name and metadata.
    pub fn read_dir(&self, path: &str) -> io::Result<impl Iterator<Item = DirEntry> + '_> {
        if !self.metadata(path)?.is_dir() {
            return Err(ErrorKind::NotADirectory.into());
        }
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let prefix_len = prefix.len();
        Ok(self
            .entries
            .range(prefix.clone()..)
            .take_while(move |(name, _)| name.starts_with(&prefix))
            .filter_map(move |(name, node)| {
                let name = &name[prefix_len..];
                if name.is_empty() || name.contains('/') {
                    return None;
                }
                Some(DirEntry {
                    name: name.to_owned(),
                    metadata: node.metadata(),
                })
            }))
    }

    /// Reads a file's complete contents.
    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        Ok(self.open(path)?.read_all())
    }

    /// Removes a file's directory entry. Existing handles keep their contents
    /// and access rights; the file's read-only flag does not prevent unlinking.
    pub fn remove_file(&mut self, path: &str) -> io::Result<()> {
        validate_path(path)?;
        self.require_parents(path)?;
        match self.entries.get(path) {
            Some(Node::File(_)) => {}
            Some(Node::Directory) => return Err(ErrorKind::IsADirectory.into()),
            None => return Err(ErrorKind::NotFound.into()),
        }
        self.entries.remove(path);
        Ok(())
    }

    /// Moves a file's directory entry, replacing any file at the destination.
    /// Parent directories must exist. Open handles retain their file's identity
    /// and access rights; read-only attributes do not prevent renaming.
    pub fn rename_file(&mut self, from: &str, to: &str) -> io::Result<()> {
        if self.metadata(from)?.is_dir() {
            return Err(ErrorKind::IsADirectory.into());
        }
        validate_path(to)?;
        self.require_parents(to)?;
        if self.is_dir(to) {
            return Err(ErrorKind::IsADirectory.into());
        }
        let file = self.entries.remove(from).expect("existing file");
        self.entries.insert(to.to_owned(), file);
        Ok(())
    }

    /// Removes an empty directory. The root directory cannot be removed.
    pub fn remove_dir(&mut self, path: &str) -> io::Result<()> {
        if path.is_empty() {
            return Err(ErrorKind::PermissionDenied.into());
        }
        if self.read_dir(path)?.next().is_some() {
            return Err(ErrorKind::DirectoryNotEmpty.into());
        }
        self.entries.remove(path);
        Ok(())
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
