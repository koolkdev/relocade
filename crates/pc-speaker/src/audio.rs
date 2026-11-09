use crate::Speaker;
use async_channel::Sender;
use futures_lite::future::or;
use relocade_machine::Clock;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

mod synthesis;
use synthesis::{Signal, Synthesis};

impl Speaker {
    /// Attaches output once. The PIT's clock drives synthesis in either host
    /// or guest time; the PCM receiver supplies playback at the configured rate.
    /// Control changes are retained until their sample interval is rendered.
    pub fn attach_audio(
        &mut self,
        config: cpal::StreamConfig,
        samples: Sender<Vec<f32>>,
    ) -> Result<(), String> {
        if self.audio.is_some() || self.audio_ready.0.is_closed() {
            return Err("speaker output is already attached".into());
        }
        if config.sample_rate == 0 || config.sample_rate > 1_000_000_000 || config.channels == 0 {
            return Err(
                "speaker output needs a representable sample rate and nonzero channel count".into(),
            );
        }
        if samples.capacity().is_none() {
            return Err("speaker output needs a bounded sample channel".into());
        }
        let mut pit = self.pit.borrow_mut();
        let changes = Rc::new(RefCell::new(VecDeque::new()));
        let recorded = Rc::downgrade(&changes);
        let control = self.control.clone();
        pit.observe_channel2(move |state| {
            if let Some(changes) = recorded.upgrade() {
                changes.borrow_mut().push_back(Signal {
                    state,
                    enabled: control.get() & 2 != 0,
                });
            }
        });
        self.audio = Some(Audio {
            clock: pit.clock().clone(),
            config,
            samples,
            changes,
        });
        self.audio_ready.0.try_send(()).unwrap();
        Ok(())
    }
}

pub(super) struct Audio {
    clock: Clock,
    config: cpal::StreamConfig,
    samples: Sender<Vec<f32>>,
    changes: Rc<RefCell<VecDeque<Signal>>>,
}

pub(super) async fn run(device: Rc<RefCell<Speaker>>) {
    let ready = device.borrow().audio_ready.1.clone();
    ready.recv().await.unwrap();
    // Attachment remains one-shot after the activity takes ownership.
    ready.close();
    let audio = device.borrow_mut().audio.take().unwrap();
    let initial = audio.changes.borrow_mut().pop_front().unwrap();
    let mut synthesis = Synthesis::new(audio.config.sample_rate, initial);
    let batch = match audio.config.buffer_size {
        cpal::BufferSize::Fixed(frames) => frames.clamp(1, 256) as usize,
        cpal::BufferSize::Default => 256,
    };
    while !audio.samples.is_closed() {
        or(audio.clock.sleep_until(synthesis.deadline(batch)), async {
            audio.samples.closed().await
        })
        .await;
        if audio.samples.is_closed() {
            return;
        }
        let buffer = synthesis.buffer(
            batch,
            usize::from(audio.config.channels),
            &mut audio.changes.borrow_mut(),
        );
        // An unplayed or stalled sink keeps recent PCM. Do not leave register
        // changes accumulating while the output channel is full.
        if audio.samples.force_send(buffer).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
