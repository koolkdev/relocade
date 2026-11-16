# Audio mixing

`Mixer` combines clocked sources into one bounded PCM stream on an execution
runner. It depends on the shared machine clock and activity interface, with no
dependency on PC hardware, BIOS or DOS. Each guest environment configures its
own sources; the application owns CPAL output and playback activation.

Create a mixer with `Mixer::new(clock, config, samples)`, passing the output's
CPAL configuration and bounded `async_channel::Sender<Vec<f32>>`. Register sources
with `add_source(gain, callback)` before connecting the activity to the runner:

```rust,ignore
let mut mixer = Mixer::new(runner.clock().clone(), config, samples)?;
let (mut recorder, mut generator) = speaker.audio_source(mixer.timing());
mixer.add_source(1.0, move |frames, output| generator.generate_samples(recorder.take_events(), frames, output));
runner.connect(Rc::new(RefCell::new(mixer)), |_| {}).await;
```

Each callback runs once per block and fills one mono sample per frame in a `Range<u64>`. Every source
uses the same immutable `AudioTiming`: a native timestamp for frame zero, the
native tick rate, and the output sample rate. Sources own synthesis and precise
placement of changes within frames. Only the block wakeup is rounded to a native
tick. The mixer applies gains, sums contributions and copies the result across
output channels. Platform output converts and clamps the final mixed sample.

Absolute frame numbers retain the sample grid across batches. Fixed buffer hints
are clamped to 1–256 frames; the default is 256. A full output channel replaces
its oldest buffer while sources continue advancing. Closing the receiver ends
synthesis and releases the sources. The execution session owns the activity's
lifetime and drops submission when it ends.
