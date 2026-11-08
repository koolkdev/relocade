//! ASCII 8.3 pathname components and search masks, following DOS NameTrans.

/// A concrete DOS directory key: eight name bytes and three extension bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShortName([u8; 11]);

impl ShortName {
    pub(crate) fn parse(component: &[u8]) -> Option<Self> {
        let fields = search_pattern(component)?;
        (!fields.contains(&b'?')).then_some(Self(fields))
    }

    /// Mounted VFS entries already have DOS names; parsing must not invent aliases
    /// for host entries whose spelling would change during pathname translation.
    pub(crate) fn from_entry(name: &str) -> Option<Self> {
        let parsed = Self::parse(name.as_bytes())?;
        (parsed.to_dotted() == name).then_some(parsed)
    }

    pub(crate) fn stem(&self) -> &[u8] {
        self.0[..8].trim_ascii_end()
    }

    pub(crate) fn to_dotted(self) -> String {
        let base = std::str::from_utf8(self.stem()).expect("ASCII short name");
        let extension =
            std::str::from_utf8(self.0[8..].trim_ascii_end()).expect("ASCII short extension");
        if extension.is_empty() {
            base.to_owned()
        } else {
            format!("{base}.{extension}")
        }
    }

    pub(crate) fn matches(&self, pattern: &[u8; 11]) -> bool {
        pattern
            .iter()
            .zip(&self.0)
            .all(|(pattern, name)| *pattern == b'?' || pattern == name)
    }
}

/// The fixed-width mask is also the representation saved in a DOS search DTA.
pub(crate) fn search_pattern(component: &[u8]) -> Option<[u8; 11]> {
    let mut result = [b' '; 11];
    if matches!(component, b"." | b"..") {
        result[..component.len()].copy_from_slice(component);
        return Some(result);
    }
    if matches!(component.first(), None | Some(b' ' | b'.')) {
        return None;
    }
    let dot = component
        .iter()
        .position(|byte| *byte == b'.')
        .unwrap_or(component.len());
    let (base, extension) = component.split_at(dot);
    field(base, &mut result[..8])?;
    field(
        extension.strip_prefix(b".").unwrap_or(extension),
        &mut result[8..],
    )?;
    Some(result)
}

fn field(input: &[u8], output: &mut [u8]) -> Option<()> {
    let mut position = 0;
    for &byte in input {
        if !(b' '..=0x7f).contains(&byte)
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
        // DOS consumes the whole field but stores only its first eight or three
        // bytes. A star fills the remaining positions, ignoring later letters.
        if position == output.len() {
            continue;
        }
        if byte == b'*' {
            output[position..].fill(b'?');
            position = output.len();
        } else {
            output[position] = byte.to_ascii_uppercase();
            position += 1;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests;
