use async_channel::Sender;
#[cfg(not(target_arch = "wasm32"))]
use relocade_app::audio::cpal::BufferSize;
use relocade_app::audio::cpal::StreamConfig;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

pub struct Output {
    #[cfg(not(target_arch = "wasm32"))]
    samples: async_channel::Receiver<Vec<f32>>,
    #[cfg(target_arch = "wasm32")]
    audio: relocade_app::audio::Audio,
    #[cfg(target_arch = "wasm32")]
    observer: browser::Observer,
    #[cfg(target_arch = "wasm32")]
    buffered_frames: u32,
}

impl Output {
    pub fn open() -> (Self, StreamConfig, Sender<Vec<f32>>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // ALSA null consumes without pacing. This fixture consumes at the
            // configured rate; CPAL's native callback has its own app tests.
            let (sender, samples) = async_channel::bounded(16);
            let config = StreamConfig {
                channels: 1,
                sample_rate: 48_000,
                buffer_size: BufferSize::Fixed(256),
            };
            (Self { samples }, config, sender)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let observer = browser::observe_output();
            let (audio, config, samples) =
                relocade_app::audio::Audio::open(|error| panic!("{error}")).unwrap();
            // Speaker batches hold at most 256 frames. Include the channel,
            // the producer's pending send and the consumer's partial batch.
            let buffered_frames = (samples.capacity().unwrap() as u32 + 2) * 256;
            (
                Self {
                    audio,
                    observer,
                    buffered_frames,
                },
                config,
                samples,
            )
        }
    }

    pub fn play(&self) {
        #[cfg(target_arch = "wasm32")]
        self.audio.play().unwrap();
    }

    pub async fn check(&self, rate: u32, frequency: u32) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut capture = Vec::new();
            for batch in 0..50 {
                futures_timer::Delay::new(Duration::from_secs_f64(256.0 / f64::from(rate))).await;
                let buffer = self.samples.try_recv().unwrap_or_else(|_| vec![0.0; 256]);
                assert_eq!(buffer.len(), 256);
                // Drain the queue and its pending send before observing a change.
                if batch >= 18 {
                    capture.extend(buffer);
                }
            }
            if frequency == 0 {
                if capture.iter().any(|sample| *sample != 0.0) {
                    return Err("expected silence".into());
                }
            } else {
                let crossings = capture
                    .windows(2)
                    .filter(|s| s[0] <= 0.1 && s[1] > 0.1)
                    .count();
                let measured = crossings as f64 * f64::from(rate) / capture.len() as f64;
                if (measured - f64::from(frequency)).abs() >= 15.0 {
                    return Err(format!("expected {frequency} Hz, measured {measured} Hz"));
                }
                if capture.iter().any(|sample| !(0.0..=0.2).contains(sample)) {
                    return Err("speaker samples exceed 0.0..=0.2".into());
                }
            }
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.observer
                .check(rate, frequency, self.buffered_frames)
                .await
                .map_err(|error| format!("{error:?}"))
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Output {
    fn drop(&mut self) {
        self.observer.finish();
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/../../tests/browser/audio-output.js")]
    extern "C" {
        pub type Observer;
        #[wasm_bindgen(js_name = observeOutput)]
        pub fn observe_output() -> Observer;
        #[wasm_bindgen(method, catch)]
        pub async fn check(
            this: &Observer,
            rate: u32,
            frequency: u32,
            buffered_frames: u32,
        ) -> Result<(), JsValue>;
        #[wasm_bindgen(method)]
        pub fn finish(this: &Observer);
    }
}
