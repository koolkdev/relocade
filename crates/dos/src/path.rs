//! DOS path normalization; mounted filesystems receive root-relative VFS paths.

pub(crate) const MAX_PATH_BYTES: usize = 128; // Includes the terminating NUL.

/// A resolved name retains its drive across later changes to the current drive.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct ResolvedPath {
    pub drive: char,
    pub relative: String,
}

pub(crate) fn drive(path: &[u8], default: char) -> Option<char> {
    match path {
        [drive, b':', ..] => drive
            .is_ascii_alphabetic()
            .then_some(char::from(drive.to_ascii_uppercase())),
        _ => Some(default),
    }
}

pub(crate) fn file_path(path: &[u8], current_directory: &str) -> Option<String> {
    if matches!(
        path.rsplit(|byte| matches!(byte, b'\\' | b'/')).next()?,
        b"" | b"." | b".."
    ) {
        return None;
    }
    let path = resolve(path, current_directory)?;
    (!path.is_empty()).then_some(path)
}

pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

pub(crate) fn resolve(path: &[u8], current_directory: &str) -> Option<String> {
    if path.is_empty() || path.len() >= MAX_PATH_BYTES || !path.is_ascii() {
        return None;
    }
    let path = std::str::from_utf8(path).ok()?;
    let path = if path.as_bytes().get(1) == Some(&b':') {
        &path[2..]
    } else {
        path
    };
    let mut components: Vec<_> = if path.starts_with(['\\', '/']) {
        Vec::new()
    } else {
        current_directory
            .split('/')
            .filter(|part| !part.is_empty())
            .collect()
    };
    for component in path.split(['\\', '/']) {
        match component {
            "" | "." => continue,
            ".." => {
                components.pop();
            }
            _ => {
                if component.bytes().any(|byte| {
                    byte < 0x20 || matches!(byte, b':' | b'"' | b'<' | b'>' | b'|' | b'*' | b'?')
                }) {
                    return None;
                }
                components.push(component);
            }
        }
    }
    let path = components.join("/").to_ascii_uppercase();
    (path.len() < MAX_PATH_BYTES).then_some(path)
}
