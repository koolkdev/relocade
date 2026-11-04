use super::{clocks_at, time_at};
use std::time::Duration;

#[test]
fn clock_edges_round_up_without_accumulating_period_error() {
    // ceil(N * 1e9 / 1193182), including the eleventh BIOS-rate tick.
    for (clocks, nanos) in [(1, 839), (65536, 54_925_402), (720896, 604_179_413)] {
        assert_eq!(time_at(clocks), Duration::from_nanos(nanos));
        assert_eq!(clocks_at(Duration::from_nanos(nanos - 1)), clocks - 1);
        assert_eq!(clocks_at(Duration::from_nanos(nanos)), clocks);
    }
    assert_eq!(time_at(103_090_924_800), Duration::from_secs(86_400));
}
