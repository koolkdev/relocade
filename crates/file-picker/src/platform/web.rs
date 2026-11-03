use crate::{Access, DirEntry, EntryKind, Metadata};
use js_sys::{Function, Object, Reflect, Uint8Array};
use std::{
    io,
    time::{Duration, UNIX_EPOCH},
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{DomException, FileSystemFileHandle, FileSystemHandle, FileSystemHandleKind};

pub(crate) type Directory = web_sys::FileSystemDirectoryHandle;
pub(crate) enum File {
    Selected(web_sys::File),
    Opened(FileSystemFileHandle),
}

fn window() -> io::Result<web_sys::Window> {
    web_sys::window().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "file pickers require a browser window",
        )
    })
}

pub(crate) async fn pick_file() -> io::Result<Option<(String, File)>> {
    if window()?
        .document()
        .and_then(|document| document.body())
        .is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "file selection requires a document body",
        ));
    }
    Ok(rfd::AsyncFileDialog::new()
        .pick_file()
        .await
        .map(|file| (file.file_name(), File::Selected(file.inner().clone()))))
}

pub(crate) async fn pick_directory(access: Access) -> io::Result<Option<(String, Directory)>> {
    let window = window()?;
    // Local binding avoids requiring web_sys_unstable_apis in every consumer.
    let picker = Reflect::get(&window, &"showDirectoryPicker".into()).map_err(browser_error)?;
    let picker = picker.dyn_into::<Function>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "directory selection requires the File System Access API",
        )
    })?;
    let options = Object::new();
    let mode = match access {
        Access::ReadOnly => "read",
        Access::ReadWrite => "readwrite",
    };
    Reflect::set(&options, &"mode".into(), &mode.into()).map_err(browser_error)?;
    let promise: js_sys::Promise<Directory> = picker
        .call1(&window, &options)
        .map_err(browser_error)?
        .unchecked_into();
    match JsFuture::from(promise).await {
        Ok(directory) => Ok(Some((directory.name(), directory))),
        Err(error)
            if error
                .dyn_ref::<DomException>()
                .is_some_and(|e| e.name() == "AbortError") =>
        {
            Ok(None)
        }
        Err(error) => Err(browser_error(error)),
    }
}

pub(crate) async fn read_dir(directory: &mut Directory) -> io::Result<Vec<DirEntry>> {
    let iterator = directory.values();
    let mut entries = Vec::new();
    loop {
        let next = JsFuture::from(iterator.next_iterator().map_err(browser_error)?)
            .await
            .map_err(browser_error)?;
        if next.done() {
            break;
        }
        let handle: FileSystemHandle = next.value().unchecked_into();
        let metadata = match handle.kind() {
            FileSystemHandleKind::File => {
                let handle: &FileSystemFileHandle = handle.unchecked_ref();
                let file = resolve(handle.get_file()).await?.unchecked_into();
                metadata(&file)
            }
            FileSystemHandleKind::Directory => Metadata {
                kind: EntryKind::Directory,
                len: 0,
                modified: None,
                created: None,
            },
            _ => unreachable!("file system handle kind"),
        };
        entries.push(DirEntry {
            name: handle.name(),
            metadata,
        });
    }
    Ok(entries)
}

pub(crate) async fn open_file(
    directory: &mut Directory,
    name: String,
    _: Access,
) -> io::Result<File> {
    Ok(File::Opened(
        resolve(directory.get_file_handle(&name))
            .await?
            .unchecked_into(),
    ))
}

pub(crate) async fn open_directory(
    directory: &mut Directory,
    name: String,
) -> io::Result<Directory> {
    Ok(resolve(directory.get_directory_handle(&name))
        .await?
        .unchecked_into())
}

async fn snapshot(file: &File) -> io::Result<web_sys::File> {
    match file {
        File::Selected(file) => Ok(file.clone()),
        File::Opened(handle) => Ok(resolve(handle.get_file()).await?.unchecked_into()),
    }
}

pub(crate) async fn read_file(
    file: &mut File,
    offset: u64,
    length: Option<usize>,
) -> io::Result<Vec<u8>> {
    let file = snapshot(file).await?;
    let start = position(offset)?;
    let end = match length {
        Some(length) => position(
            offset
                .checked_add(length as u64)
                .ok_or(io::ErrorKind::InvalidInput)?,
        )?,
        None => file.size(),
    };
    let blob = file
        .slice_with_f64_and_f64(start, end)
        .map_err(browser_error)?;
    Ok(Uint8Array::new(&resolve(blob.array_buffer()).await?).to_vec())
}

pub(crate) async fn file_metadata(file: &mut File) -> io::Result<Metadata> {
    Ok(metadata(&snapshot(file).await?))
}

fn metadata(file: &web_sys::File) -> Metadata {
    let milliseconds = file.last_modified() as i64;
    let duration = Duration::from_millis(milliseconds.unsigned_abs());
    let modified = if milliseconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    };
    Metadata {
        kind: EntryKind::File,
        len: file.size() as u64,
        modified,
        created: None,
    }
}

fn position(value: u64) -> io::Result<f64> {
    if value > (1u64 << 53) - 1 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(value as f64)
}

async fn resolve(promise: js_sys::Promise) -> io::Result<JsValue> {
    JsFuture::from(promise).await.map_err(browser_error)
}

fn browser_error(error: JsValue) -> io::Error {
    if let Some(exception) = error.dyn_ref::<DomException>() {
        let kind = match exception.name().as_str() {
            "NotAllowedError" | "SecurityError" => io::ErrorKind::PermissionDenied,
            "NotSupportedError" => io::ErrorKind::Unsupported,
            "NotFoundError" => io::ErrorKind::NotFound,
            "TypeMismatchError" => io::ErrorKind::InvalidInput,
            _ => io::ErrorKind::Other,
        };
        io::Error::new(
            kind,
            format!("{}: {}", exception.name(), exception.message()),
        )
    } else {
        io::Error::other(format!("{error:?}"))
    }
}
