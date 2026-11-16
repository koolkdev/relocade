use super::*;
use relocade_time::{TickRate, Timestamp};
use std::{cell::Cell, num::NonZeroU64, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn local_recorders_preserve_generator_state_and_interleaved_mix_headroom() {
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
    let mut output = [99.0; 4];
    for (block, expected) in [[1.5, 1.5, 0.5, 0.5], [2.5, 2.5, 1.5, 1.5]]
        .into_iter()
        .enumerate()
    {
        let start = block as u64 * 2;
        sources.generate_samples(start..start + 2, &mut output);
        assert_eq!(output, expected);
    }
    assert_eq!(Rc::strong_count(&levels), 2);
    drop(sources);
    assert_eq!(Rc::strong_count(&levels), 1);
}
