use crate::{Access, DirEntry, EntryKind, Metadata};
use blocking::Unblock;
use cap_std::{ambient_authority, fs};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

pub(crate) type Directory = Unblock<fs::Dir>;
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
            &file_options(Access::ReadOnly, false),
            ambient_authority(),
        )?)
    })
    .await?;
    Ok((name, file))
}

pub(crate) async fn pick_directory(_: Access) -> io::Result<Option<(String, Directory)>> {
    let Some(selection) = rfd::AsyncFileDialog::new().pick_folder().await else {
        return Ok(None);
    };
    selected_directory(selection.into()).await.map(Some)
}

pub(crate) async fn selected_directory(path: PathBuf) -> io::Result<(String, Directory)> {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    let directory =
        blocking::unblock(move || fs::Dir::open_ambient_dir(path, ambient_authority())).await?;
    Ok((name, Unblock::new(directory)))
}

pub(crate) async fn read_dir(directory: &mut Directory) -> io::Result<Vec<DirEntry>> {
    directory
        .with_mut(|directory| {
            let mut entries = Vec::new();
            for entry in directory.entries()? {
                let entry = entry?;
                let stat = entry.metadata()?;
                let kind = if stat.is_file() {
                    EntryKind::File
                } else if stat.is_dir() {
                    EntryKind::Directory
                } else {
                    continue;
                };
                let name = entry.file_name().into_string().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 entry name")
                })?;
                entries.push(DirEntry {
                    name,
                    metadata: metadata(stat, kind),
                });
            }
            Ok(entries)
        })
        .await
}

pub(crate) async fn open_file(
    directory: &mut Directory,
    name: String,
    access: Access,
    create: bool,
) -> io::Result<File> {
    directory
        .with_mut(move |directory| {
            regular_file(directory.open_with(name, &file_options(access, create))?)
        })
        .await
}

pub(crate) async fn open_directory(
    directory: &mut Directory,
    name: String,
    create: bool,
) -> io::Result<Directory> {
    directory
        .with_mut(move |directory| {
            if create {
                match directory.create_dir(&name) {
                    Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                        return Err(error)
                    }
                    _ => {}
                }
            }
            directory.open_dir(name).map(Unblock::new)
        })
        .await
}

fn file_options(access: Access, create: bool) -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .write(access == Access::ReadWrite)
        .create(create);
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

pub(crate) async fn file_metadata(file: &mut File) -> io::Result<Metadata> {
    file.with_mut(|file| Ok(metadata(file.metadata()?, EntryKind::File)))
        .await
}

fn metadata(stat: fs::Metadata, kind: EntryKind) -> Metadata {
    Metadata {
        kind,
        len: if kind == EntryKind::File {
            stat.len()
        } else {
            0
        },
        modified: stat.modified().ok().map(|time| time.into_std()),
        created: stat.created().ok().map(|time| time.into_std()),
    }
}

pub(crate) async fn write_file(
    file: &mut File,
    offset: u64,
    bytes: Vec<u8>,
    replace: bool,
) -> io::Result<()> {
    file.with_mut(move |file| {
        file.seek(SeekFrom::Start(offset))?;
        if replace {
            file.set_len(0)?;
        }
        file.write_all(&bytes)
    })
    .await
}

pub(crate) async fn set_len(file: &mut File, length: u64) -> io::Result<()> {
    file.with_mut(move |file| file.set_len(length)).await
}
