use crate::{
    platform,
    service::{Resource, Resources},
    Access, File, Metadata,
};
use std::io;

/// A directory entry observed without reading file contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    pub name: String,
    pub metadata: Metadata,
}

/// Rust-only connection to a directory owned by the file service.
/// Child lookups accept one UTF-8 name, not a path. Open directories to descend.
/// Clones share the resource; dropping the last handle releases it on its owner.
#[derive(Clone, Debug)]
pub struct Directory {
    name: String,
    access: Access,
    resource: Resource<DirectoryOwner>,
}

struct DirectoryOwner {
    directory: platform::Directory,
    resources: Resources,
}

impl Directory {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn access(&self) -> Access {
        self.access
    }

    /// Lists immediate files/directories in name order. Native symbolic links
    /// and special files are omitted; non-portable names return `InvalidData`.
    /// Metadata is collected per entry; the listing is not an atomic snapshot
    /// of the whole directory.
    pub async fn read_dir(&self) -> io::Result<Vec<DirEntry>> {
        self.resource
            .call(async |owner| {
                let mut entries = platform::read_dir(&mut owner.directory).await?;
                for entry in &entries {
                    validate_name(&entry.name).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "unsupported entry name")
                    })?;
                }
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                Ok(entries)
            })
            .await
    }

    /// Opens an existing file without truncation. Access cannot exceed the parent's.
    pub async fn open_file(&self, name: &str, access: Access) -> io::Result<File> {
        self.file(name, access, false).await
    }

    /// Opens or creates a read/write file, preserving any existing contents.
    pub async fn create_file(&self, name: &str) -> io::Result<File> {
        self.file(name, Access::ReadWrite, true).await
    }

    /// Opens an existing subdirectory with this directory's access.
    pub async fn open_directory(&self, name: &str) -> io::Result<Self> {
        self.directory(name, false).await
    }

    /// Opens or creates a subdirectory. Requires read/write access.
    pub async fn create_directory(&self, name: &str) -> io::Result<Self> {
        self.directory(name, true).await
    }

    async fn file(&self, name: &str, access: Access, create: bool) -> io::Result<File> {
        let name = name.to_owned();
        let parent_access = self.access;
        self.resource
            .call(async move |owner| {
                validate_name(&name)?;
                parent_access.require(access)?;
                let file =
                    platform::open_file(&mut owner.directory, name.clone(), access, create).await?;
                owner.resources.register_file(name, access, file)
            })
            .await
    }

    async fn directory(&self, name: &str, create: bool) -> io::Result<Self> {
        let name = name.to_owned();
        let access = self.access;
        self.resource
            .call(async move |owner| {
                validate_name(&name)?;
                if create {
                    access.require(Access::ReadWrite)?;
                }
                let directory =
                    platform::open_directory(&mut owner.directory, name.clone(), create).await?;
                owner.resources.register_directory(name, access, directory)
            })
            .await
    }
}

impl Resources {
    pub(crate) fn register_directory(
        &self,
        name: String,
        access: Access,
        directory: platform::Directory,
    ) -> io::Result<Directory> {
        Ok(Directory {
            name,
            access,
            resource: self.register(DirectoryOwner {
                directory,
                resources: self.clone(),
            })?,
        })
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    if matches!(name, "" | "." | "..") || name.contains(['/', '\\', '\0', ':']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one child name",
        ));
    }
    Ok(())
}
