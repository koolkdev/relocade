# Audio mixing

`Mixer` combines clocked sources into one bounded PCM stream on an execution
runner. It depends on the shared machine clock and activity interface, with no
dependency on PC hardware, BIOS or DOS. Each guest environment configures its
own sources; the application owns CPAL output and playback activation.

Create a mixer with `Mixer::new(clock, config, samples)`, passing the output's
CPAL configuration and bounded `async_channel::Sender<Vec<f32>>`. Register devices
with the mixer's `Sources` collection before connecting the activity:

```rust,ignore
let mut mixer = Mixer::new(runner.clock().clone(), config, samples)?;
speaker.add_audio_sources(mixer.sources());
runner.connect(Rc::new(RefCell::new(mixer)), |_| {}).await;
```

Devices register their own recorder/generator pairs on the collection's shared
`AudioTiming`. The collection records events and mixes all sources once per block
of absolute frames, retaining independent gains and headroom. The mixer owns
wakeup scheduling and bounded PCM submission; platform output clamps samples.

Absolute frame numbers retain the sample grid across batches. Fixed buffer hints
are clamped to 1–256 frames; the default is 256. A full output channel replaces
its oldest buffer while sources continue advancing. Closing the receiver ends
synthesis and releases the sources. The execution session owns the activity's
lifetime and drops submission when it ends.
