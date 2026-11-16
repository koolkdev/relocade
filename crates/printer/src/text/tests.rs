use super::{layout, render_pdf};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cp437_positioning_and_overprinting_have_platform_independent_ink() {
    // Full block glyphs make the expected bytes independent of font rendering.
    let pages = layout(b"\xdb\t\xdb\r\n \xdb\n\xdb\x08 \x0c\xdb").unwrap();
    assert_eq!(pages.len(), 2);
    let mut expected = vec![0; 80 * 480];
    for y in 0..8 {
        expected[y * 80] = 0xff;
        expected[y * 80 + 8] = 0xff;
        expected[(y + 8) * 80 + 1] = 0xff;
        expected[(y + 16) * 80 + 2] = 0xff; // LF preserves the column; space cannot erase ink.
    }
    assert_eq!(pages[0], expected);
    let mut second = vec![0; 80 * 480];
    for y in 0..8 {
        second[y * 80] = 0xff;
    }
    assert_eq!(pages[1], second);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn page_boundaries_preserve_blank_pages_without_a_trailing_phantom_page() {
    assert_eq!(layout(b"A\x0c").unwrap().len(), 1);
    assert_eq!(layout(b"\x0c\x0c").unwrap().len(), 2);
    let mut bytes = vec![0xdb; 80 * 60];
    let full = layout(&bytes).unwrap();
    assert_eq!(full.len(), 1);
    assert!(full[0].iter().all(|&byte| byte == 0xff));
    bytes.push(0xdb);
    let wrapped = layout(&bytes).unwrap();
    assert_eq!(wrapped.len(), 2);
    assert_eq!(wrapped[1][0], 0xff);
    assert_eq!(wrapped[1][1], 0);
    assert_eq!(layout(&b"\r\n".repeat(60)).unwrap().len(), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unsupported_commands_and_excessive_blank_pages_fail_before_saving() {
    assert!(render_pdf(b"").is_err());
    assert!(render_pdf(b"\0\x07").is_err());
    assert!(render_pdf(b"hello\x1b@").unwrap_err().contains("0x1B"));
    assert_eq!(layout(&[0x0c; 64]).unwrap().len(), 64);
    assert!(render_pdf(&[0x0c; 65]).unwrap_err().contains("64 pages"));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pdf_describes_two_letter_pages_with_one_bit_ink() {
    let pdf = render_pdf(b"A\x0cB").unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    let objects = String::from_utf8_lossy(&pdf);
    assert!(objects.contains("/Count 2"));
    assert_eq!(objects.matches("/MediaBox [0 0 612 792]").count(), 2);
    assert_eq!(objects.matches("/BitsPerComponent 1").count(), 2);
    assert_eq!(objects.matches("/Decode [1 0]").count(), 2);
    assert_eq!(objects.matches("540 0 0 720 36 36 cm").count(), 2);
}
