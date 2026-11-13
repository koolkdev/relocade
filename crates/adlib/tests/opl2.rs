use relocade_adlib::Opl2;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn tone(chip: &mut Opl2) {
    for (register, value) in [
        (0x20, 0x21), // Sustaining operators, frequency multiplier 1.
        (0x23, 0x21),
        (0x40, 0x3f), // Silent modulator, audible carrier.
        (0x43, 0x00),
        (0x60, 0xf0), // Immediate attack, no decay.
        (0x63, 0xf0),
        (0x80, 0x0f), // Fast release.
        (0x83, 0x0f),
        (0xa0, 0x98), // F-number 408, block 4: approximately 309.5 Hz.
        (0xb0, 0x31),
    ] {
        chip.write_register(register, value);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn mono_tone_keeps_pitch_at_both_output_rates_and_releases() {
    for rate in [44_100, 48_000] {
        let mut chip = Opl2::new(rate).unwrap();
        assert!((0..100).all(|_| chip.sample() == 0.0));
        tone(&mut chip);
        let samples: Vec<_> = (0..rate / 5).map(|_| chip.sample()).collect();
        assert!(samples.iter().all(|s| (-1.0..=1.0).contains(s)));
        let edges = samples
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        assert!((60..=63).contains(&edges), "rate {rate}: {edges} cycles");
        chip.write_register(0xb0, 0x11);
        for _ in 0..rate / 5 {
            chip.sample();
        }
        assert!((0..100).all(|_| chip.sample().abs() < 0.001));
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn waveform_selection_requires_the_ym3812_enable_bit() {
    let mut sine = Opl2::new(48_000).unwrap();
    let mut selected = Opl2::new(48_000).unwrap();
    tone(&mut sine);
    tone(&mut selected);
    selected.write_register(0xe3, 1);
    for _ in 0..1000 {
        assert_eq!(sine.sample(), selected.sample());
    }
    selected.write_register(0x01, 0x20);
    let differs = (0..1000).any(|_| sine.sample() != selected.sample());
    assert!(differs);
}
