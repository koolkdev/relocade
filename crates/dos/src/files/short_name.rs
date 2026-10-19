//! DOS searches compare space-padded 8.3 names, including '?' padding matches.

pub(super) fn name(name: &[u8]) -> Option<[u8; 11]> {
    if name.contains(&b'*') || name.contains(&b'?') {
        return None;
    }
    pattern(name)
}

pub(super) fn pattern(name: &[u8]) -> Option<[u8; 11]> {
    let mut result = [b' '; 11];
    if matches!(name, b"." | b"..") {
        result[..name.len()].copy_from_slice(name);
        return Some(result);
    }
    let dot = name
        .iter()
        .position(|byte| *byte == b'.')
        .unwrap_or(name.len());
    let (base, extension) = name.split_at(dot);
    if base.is_empty() {
        return None;
    }
    field(base, &mut result[..8])?;
    field(
        extension.strip_prefix(b".").unwrap_or(extension),
        &mut result[8..],
    )?;
    Some(result)
}

fn field(input: &[u8], output: &mut [u8]) -> Option<()> {
    for (index, byte) in input.iter().copied().enumerate() {
        if byte == b'*' {
            output[index..].fill(b'?');
            return Some(());
        }
        if index >= output.len() {
            return None;
        }
        if !byte.is_ascii_graphic()
            || matches!(
                byte,
                b'.' | b'"'
                    | b'/'
                    | b'\\'
                    | b':'
                    | b';'
                    | b','
                    | b'+'
                    | b'='
                    | b'['
                    | b']'
                    | b'<'
                    | b'>'
                    | b'|'
            )
        {
            return None;
        }
        output[index] = byte.to_ascii_uppercase();
    }
    Some(())
}

pub(super) fn matches(pattern: &[u8; 11], name: &[u8; 11]) -> bool {
    pattern
        .iter()
        .zip(name)
        .all(|(pattern, name)| *pattern == b'?' || pattern == name)
}
