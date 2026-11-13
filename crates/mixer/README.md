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
mixer.add_source(1.0, move |interval| source.sample(interval));
runner.connect(Rc::new(RefCell::new(mixer)), |_| {}).await;
```

The current source interface is mono: each callback averages its signal over
the supplied `Range<Timestamp>` on the mixer's clock. Sources own their synthesis,
timed changes and rate conversion. Every source receives the same elapsed
interval, including silent sources. The mixer applies gains, sums contributions
and duplicates the result across output channels. Platform output converts and
clamps the final mixed sample.

One frame cursor retains the sample timeline across batches. Fixed buffer hints
are clamped to 1–256 frames; the default is 256. A full output channel replaces
its oldest buffer while sources continue advancing. Closing the receiver ends
synthesis and releases the sources. The execution session owns the activity's
lifetime and drops submission when it ends.
