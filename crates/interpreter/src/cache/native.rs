use std::path::PathBuf;

pub(super) struct Storage {
    pub(super) directory: PathBuf,
}

impl Storage {
    pub(super) async fn open() -> Option<Self> {
        let directories = directories_next::ProjectDirs::from("", "", "relocade")?;
        Some(Self {
            directory: directories.cache_dir().join("interpreters"),
        })
    }

    pub(super) async fn read(&self, key: &str) -> Option<Vec<u8>> {
        cacache::read_sync(&self.directory, key).ok()
    }

    pub(super) async fn write(&self, key: &str, bytes: &[u8]) {
        let _ = cacache::write_sync(&self.directory, key, bytes);
    }
}
