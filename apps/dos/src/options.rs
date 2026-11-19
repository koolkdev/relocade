use std::num::NonZeroU32;

fn parse_mips(value: &str) -> Result<NonZeroU32, String> {
    value
        .parse()
        .map_err(|_| format!("expected a positive whole number of MIPS, got {value:?}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn max_mips() -> Result<Option<NonZeroU32>, String> {
    let mut arguments = std::env::args_os().skip(1);
    match (arguments.next(), arguments.next(), arguments.next()) {
        (None, None, None) => Ok(None),
        (Some(flag), Some(value), None) if flag == "--mips" => {
            parse_mips(&value.to_string_lossy()).map(Some)
        }
        _ => Err("usage: relocade-dos [--mips N]".into()),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn max_mips() -> Result<Option<NonZeroU32>, String> {
    let window = web_sys::window().ok_or("the DOS console needs a browser window")?;
    let query = window
        .location()
        .search()
        .map_err(|error| format!("{error:?}"))?;
    let parameters =
        web_sys::UrlSearchParams::new_with_str(&query).map_err(|error| format!("{error:?}"))?;
    parameters
        .get("mips")
        .as_deref()
        .map(parse_mips)
        .transpose()
}
