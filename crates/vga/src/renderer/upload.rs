//! Mirror queued GPU bytes independently of image decoding and submission.
use std::ops::Range;

pub(super) struct QueuedBytes {
    bytes: Vec<u8>,
    initialized: Vec<bool>,
}

impl QueuedBytes {
    pub fn new(size: usize) -> Self {
        assert_eq!(size % 4, 0);
        Self {
            bytes: vec![0; size],
            initialized: vec![false; size / 4],
        }
    }

    /// The mask selects four-byte words in a slice of at most 256 bytes. Only
    /// changed selected words are transferred. The callback queues writes;
    /// discarding a subsequent draw encoder does not discard them.
    pub fn update(
        &mut self,
        offset: usize,
        bytes: &[u8],
        mut required: u64,
        mut write: impl FnMut(usize, &[u8]),
    ) {
        assert_eq!(offset % 4, 0);
        assert_eq!(bytes.len() % 4, 0);
        assert!(bytes.len() <= 256);
        let mut run: Option<Range<usize>> = None;
        while required != 0 {
            let word = required.trailing_zeros() as usize;
            required &= required - 1;
            let start = word * 4;
            let range = offset + start..offset + start + 4;
            let initialized = &mut self.initialized[offset / 4 + word];
            let changed = !*initialized || self.bytes[range.clone()] != bytes[start..start + 4];
            if run.as_ref().is_some_and(|run| !changed || run.end != start) {
                let run = run.take().unwrap();
                write(offset + run.start, &bytes[run]);
            }
            if changed {
                *initialized = true;
                self.bytes[range].copy_from_slice(&bytes[start..start + 4]);
                if let Some(run) = &mut run {
                    run.end = start + 4;
                } else {
                    run = Some(start..start + 4);
                }
            }
        }
        if let Some(run) = run {
            write(offset + run.start, &bytes[run]);
        }
    }
}

pub(super) struct InputBuffer {
    pub buffer: wgpu::Buffer,
    contents: QueuedBytes,
}

impl InputBuffer {
    pub fn new(device: &wgpu::Device, size: usize, usage: wgpu::BufferUsages) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("captured VGA input"),
                size: size as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            contents: QueuedBytes::new(size),
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, bytes: &[u8]) {
        for (chunk, bytes) in bytes.chunks(256).enumerate() {
            let required = u64::MAX >> (64 - bytes.len() / 4);
            self.contents
                .update(chunk * 256, bytes, required, |offset, bytes| {
                    queue.write_buffer(&self.buffer, offset as u64, bytes);
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QueuedBytes;

    #[test]
    fn uploads_only_changed_required_words_and_coalesces_adjacent_changes() {
        let mut contents = QueuedBytes::new(24);
        let required = 0b11_1111;
        let mut bytes = [0; 24];
        let mut writes = Vec::new();
        contents.update(0, &bytes, required, |offset, bytes| {
            writes.push((offset, bytes.to_vec()))
        });
        assert_eq!(writes, [(0, vec![0; 24])]);
        writes.clear();
        contents.update(0, &bytes, required, |offset, bytes| {
            writes.push((offset, bytes.to_vec()))
        });
        assert!(writes.is_empty());
        bytes[1] = 7;
        bytes[6] = 9;
        bytes[21] = 11;
        contents.update(0, &bytes, required, |offset, bytes| {
            writes.push((offset, bytes.to_vec()))
        });
        assert_eq!(
            writes,
            [(0, vec![0, 7, 0, 0, 0, 0, 9, 0]), (20, vec![0, 11, 0, 0])]
        );
    }

    #[test]
    fn hidden_changes_survive_skips_and_older_captures_restore_resident_bytes() {
        let mut contents = QueuedBytes::new(16);
        let old = [1; 8];
        contents.update(8, &old, 0b01, |_, _| {});
        let newest = [2, 1, 1, 1, 3, 1, 1, 1];
        let mut writes = Vec::new();
        contents.update(8, &newest, 0b10, |offset, bytes| {
            writes.push((offset, bytes.to_vec()))
        });
        assert_eq!(writes, [(12, vec![3, 1, 1, 1])]);
        writes.clear();
        contents.update(8, &newest, 0b11, |offset, bytes| {
            writes.push((offset, bytes.to_vec()))
        });
        assert_eq!(writes, [(8, vec![2, 1, 1, 1])]);
        writes.clear();
        contents.update(8, &old, 0b11, |offset, bytes| {
            writes.push((offset, bytes.to_vec()))
        });
        assert_eq!(writes, [(8, old.to_vec())]);
    }
}
