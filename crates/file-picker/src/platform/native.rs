use crate::{Access, FileMetadata};
use blocking::Unblock;
use cap_std::{ambient_authority, fs};
use std::{
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
};

pub(crate) type File = Unblock<fs::File>;

pub(crate) async fn pick_file() -> io::Result<Option<(String, File)>> {
    let Some(selection) = rfd::AsyncFileDialog::new().pick_file().await else {
        return Ok(None);
    };
    selected_file(selection.into()).await.map(Some)
}

pub(crate) async fn selected_file(path: PathBuf) -> io::Result<(String, File)> {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    let file = blocking::unblock(move || {
        regular_file(fs::File::open_ambient_with(
            path,
            &file_options(Access::ReadOnly),
            ambient_authority(),
        )?)
    })
    .await?;
    Ok((name, file))
}

fn file_options(access: Access) -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(access == Access::ReadWrite);
    #[cfg(unix)]
    {
        use fs::OpenOptionsExt;
        // A FIFO must not block open before we can reject it. Regular files
        // ignore this flag; their I/O still runs on the blocking pool.
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    options
}

fn regular_file(file: fs::File) -> io::Result<File> {
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(Unblock::new(file))
}

pub(crate) async fn read_file(
    file: &mut File,
    offset: u64,
    length: Option<usize>,
) -> io::Result<Vec<u8>> {
    file.with_mut(move |file| {
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        match length {
            Some(length) => file.take(length as u64).read_to_end(&mut bytes)?,
            None => file.read_to_end(&mut bytes)?,
        };
        Ok(bytes)
    })
    .await
}

pub(crate) async fn file_metadata(file: &mut File) -> io::Result<FileMetadata> {
    file.with_mut(|file| {
        let metadata = file.metadata()?;
        Ok(FileMetadata {
            len: metadata.len(),
            modified: metadata.modified().ok().map(|time| time.into_std()),
        })
    })
    .await
}
