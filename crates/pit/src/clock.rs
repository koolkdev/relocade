use std::time::Duration;

/// Nominal PC/AT PIT input frequency.
pub const INPUT_HZ: u64 = 1_193_182;

/// Completed input clocks after the given elapsed time since PIT initialization.
pub fn clocks_at(time: Duration) -> u64 {
    (time.as_nanos() * u128::from(INPUT_HZ) / 1_000_000_000)
        .try_into()
        .expect("PIT time fits in u64 input clocks")
}

/// Earliest representable time at or after an input-clock boundary.
/// Convert the absolute clock index rather than adding rounded periods.
pub fn time_at(clocks: u64) -> Duration {
    Duration::new(
        clocks / INPUT_HZ,
        ((clocks % INPUT_HZ) * 1_000_000_000).div_ceil(INPUT_HZ) as u32,
    )
}

#[cfg(test)]
mod tests;
