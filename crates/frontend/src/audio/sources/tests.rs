use super::*;
use relocade_time::{TickRate, Timestamp};
use std::{cell::Cell, num::NonZeroU64, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn recorded_calculations_preserve_generator_state_and_interleaved_mix_headroom() {
    let timing = AudioTiming::new(
        Timestamp::from_ticks(0),
        TickRate::new(NonZeroU64::new(1000).unwrap()),
        1000,
    )
    .unwrap();
    let mut sources = Sources::new(timing);
    let levels = Rc::new(Cell::new(Some([1.0, -1.0])));
    let recorded = levels.clone();
    let mut held = [0.0; 2];
    sources.add_source(
        0.5,
        move || recorded.take(),
        move |events, frames, output| {
            assert_eq!(frames.end - frames.start, 2);
            if let Some(levels) = events {
                held = levels;
            }
            output.copy_from_slice(&held);
        },
    );
    let mut count = 0u32;
    sources.add_source(
        2.0,
        || 1u32,
        move |events, _, output| {
            count += events;
            output.fill(count as f32 * 0.5);
        },
    );
    for (block, expected) in [[1.5, 1.5, 0.5, 0.5], [2.5, 2.5, 1.5, 1.5]]
        .into_iter()
        .enumerate()
    {
        let start = block as u64 * 2;
        fn require_send<T: Send>(value: T) -> T {
            value
        }
        let calculate = require_send(sources.record(start..start + 2, 2));
        let output = calculate();
        assert_eq!(output, expected);
    }
    assert_eq!(Rc::strong_count(&levels), 2);
    drop(sources);
    assert_eq!(Rc::strong_count(&levels), 1);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn dropping_sources_stops_recording_while_owned_work_can_finish() {
    let timing = AudioTiming::new(
        Timestamp::from_ticks(0),
        TickRate::new(NonZeroU64::new(1000).unwrap()),
        1000,
    )
    .unwrap();
    let mut sources = Sources::new(timing);
    let recordings = Rc::new(Cell::new(0));
    let recorded = recordings.clone();
    sources.add_source(
        1.0,
        move || recorded.set(recorded.get() + 1),
        |(), _, output| output.fill(0.0),
    );
    let calculate = sources.record(0..2, 1);
    assert_eq!(recordings.get(), 1);
    assert_eq!(Rc::strong_count(&recordings), 2);
    drop(sources);
    assert_eq!(Rc::strong_count(&recordings), 1);
    assert_eq!(calculate(), [0.0; 2]);
}
