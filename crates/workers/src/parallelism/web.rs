use std::{io, num::NonZeroUsize};

/// Returns the browser's logical processor count hint, on the window or a worker.
pub fn available_parallelism() -> io::Result<NonZeroUsize> {
    let count = js_sys::Reflect::get(&js_sys::global(), &"navigator".into())
        .ok()
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &"hardwareConcurrency".into()).ok())
        .and_then(|count| count.as_f64())
        .and_then(|count| NonZeroUsize::new(count as usize));
    count.ok_or_else(|| io::ErrorKind::Unsupported.into())
}
