use crate::FileMetadata;
use js_sys::Uint8Array;
use std::{
    io,
    time::{Duration, UNIX_EPOCH},
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::DomException;

pub(crate) type File = web_sys::File;

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
        .map(|file| (file.file_name(), file.inner().clone())))
}

pub(crate) async fn read_file(
    file: &mut File,
    offset: u64,
    length: Option<usize>,
) -> io::Result<Vec<u8>> {
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

pub(crate) async fn file_metadata(file: &mut File) -> io::Result<FileMetadata> {
    let milliseconds = file.last_modified() as i64;
    let duration = Duration::from_millis(milliseconds.unsigned_abs());
    let modified = if milliseconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    };
    Ok(FileMetadata {
        len: file.size() as u64,
        modified,
    })
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
