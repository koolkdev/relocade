use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precision_changes_preserve_access_position_and_shift_components() {
    let mut dac = Dac {
        mask: 0xa5,
        ..Dac::default()
    };
    dac.select(255, false);
    dac.write(0xff);
    assert_eq!(dac.colors[255][0], 63);
    dac.set_bits_per_component(8);
    assert_eq!(dac.colors[255][0], 252);
    dac.write(129);
    dac.write(255);
    dac.write(200); // The completed blue component advanced to entry zero.
    assert_eq!(dac.colors[255], [252, 129, 255]);
    assert_eq!(dac.colors[0][0], 200);
    assert_eq!(dac.mask, 0xa5);

    dac.select(255, true);
    assert_eq!(dac.read(), 252);
    let encoded = dac.to_state_bytes();
    assert_eq!(encoded[3], 3); // Eight-bit, reading, next component is green.
    let mut restored = Dac::from_state_bytes(&encoded).unwrap();
    assert_eq!(restored, dac);
    assert_eq!(restored.read(), 129);
    restored.set_bits_per_component(6);
    assert_eq!(restored.read(), 63);
    assert_eq!(restored.read(), 50);
    assert_eq!(restored.colors[255], [63, 32, 63]);
    let saved = restored.clone();
    restored.set_bits_per_component(6);
    assert_eq!(restored, saved);
    assert_eq!(
        Dac::from_state_bytes(&restored.to_state_bytes()),
        Some(restored)
    );
    let mut invalid = encoded;
    invalid[3] = 1; // Six-bit state cannot contain eight-bit components.
    assert!(Dac::from_state_bytes(&invalid).is_none());
}
