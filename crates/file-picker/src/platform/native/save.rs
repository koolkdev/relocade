#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::{self, Read},
    path::Path,
};
use tempfile::NamedTempFile;

pub(crate) async fn save_file(name: &str, contents: Vec<u8>) -> io::Result<Option<()>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name(name)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    blocking::unblock(move || replace_file(file.path(), contents.as_slice()))
        .await
        .map(Some)
}

fn replace_file(path: &Path, mut contents: impl Read) -> io::Result<()> {
    let permissions = match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a regular file",
            ));
        }
        Ok(metadata) if metadata.permissions().readonly() => {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(metadata) => {
            let permissions = metadata.permissions();
            #[cfg(unix)]
            let permissions = fs::Permissions::from_mode(permissions.mode() & 0o777);
            Some(permissions)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let directory = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let mut temporary = NamedTempFile::new_in(directory)?;
    io::copy(&mut contents, &mut temporary)?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_save_preserves_the_existing_file_and_can_be_retried() {
        struct FailedRead;
        impl Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("interrupted source"))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.bin");
        replace_file(&path, b"original".as_slice()).unwrap();
        let partial = b"partial".as_slice().chain(FailedRead);
        assert!(replace_file(&path, partial).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        replace_file(&path, [0, 0xff].as_slice()).unwrap();
        assert_eq!(fs::read(path).unwrap(), [0, 0xff]);
    }
}
