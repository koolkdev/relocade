use crate::{
    platform,
    service::{Resource, Resources},
    Access, Metadata,
};
use std::io;

/// Rust-only connection to a file. All requests execute on the file service.
/// Clones share a serial request queue and have no shared seek cursor.
#[derive(Clone, Debug)]
pub struct File {
    name: String,
    access: Access,
    resource: Resource<platform::File>,
}

impl File {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn access(&self) -> Access {
        self.access
    }

    /// Reads the complete file from the beginning each time.
    pub async fn read(&self) -> io::Result<Vec<u8>> {
        self.resource
            .call(async |file| platform::read_file(file, 0, None).await)
            .await
    }

    /// Reads up to the buffer length at an independent byte offset. EOF returns zero.
    pub async fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        let length = buffer.len();
        let bytes = self
            .resource
            .call(async move |file| platform::read_file(file, offset, Some(length)).await)
            .await?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }

    /// Reads size and available timestamps together from the platform file.
    pub async fn metadata(&self) -> io::Result<Metadata> {
        self.resource.call(platform::file_metadata).await
    }

    pub async fn len(&self) -> io::Result<u64> {
        Ok(self.metadata().await?.len)
    }

    pub async fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len().await? == 0)
    }
}

impl Resources {
    pub(crate) fn register_file(
        &self,
        name: String,
        access: Access,
        file: platform::File,
    ) -> io::Result<File> {
        Ok(File {
            name,
            access,
            resource: self.register(file)?,
        })
    }
}
