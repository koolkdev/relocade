# Audio output

`relocade-audio` supplies bounded interleaved PCM transport through the existing
`async-channel` crate. The application owns the producer, synthesis, scheduling
and output lifetime; this crate has no dependency on emulation or a worker framework.

```rust
use relocade_audio::{pcm_buffer, Format};

let format = Format::new(48_000, 2)?;
let (mut writer, mut reader) = pcm_buffer(format, 2048)?;
writer.try_write(&[0.25, -0.25])?;
let mut samples = [0.0; 4];
assert_eq!(reader.read(&mut samples)?, 1);
assert_eq!(samples, [0.25, -0.25, 0.0, 0.0]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The unique writer can move to a synthesis worker. It checks capacity before
sending a complete batch; a full channel reports backpressure. The reader takes
complete frames with `try_recv` and fills an underrun with silence.
Dropping the writer preserves queued samples; dropping the reader closes
submission. A concurrent output closure may interrupt an in-flight write.

Samples use the explicit rate and channel order. The wrapper validates frame
boundaries and normalizes samples; channel storage, synchronization and closure
belong to `async-channel`. There is no resampling, synthesis or playback clock.
