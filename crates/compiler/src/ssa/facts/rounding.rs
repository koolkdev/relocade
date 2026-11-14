//! Rounding facts are transferred without constructing the discarded-bit graph.
use super::Bits;
use crate::expression::rounding::{AWAY, GUARD, NEAREST, STICKY};

pub(super) fn infer(integer: Bits, distance: Bits, control: Bits, component: usize) -> Bits {
    let flag = |mask| control.known(u64::from(mask)).map(|value| value != 0);
    let original_guard = flag(GUARD);
    let original_sticky = flag(STICKY);
    let (floor, guard, sticky) = match distance.known(u64::from(u32::MAX)) {
        Some(0) => (integer, original_guard, original_sticky),
        Some(distance @ 1..=64) => {
            let guard_mask = 1_u64 << (distance - 1);
            let floor = Bits {
                zero: integer.zero.checked_shr(distance as u32).unwrap_or(0)
                    | !u64::MAX.checked_shr(distance as u32).unwrap_or(0),
                one: integer.one.checked_shr(distance as u32).unwrap_or(0),
            };
            (
                floor,
                nonzero(integer, guard_mask),
                or(
                    or(original_sticky, original_guard),
                    nonzero(integer, guard_mask - 1),
                ),
            )
        }
        Some(_) => (
            Bits::constant(0, u64::MAX),
            Some(false),
            or(
                or(original_sticky, original_guard),
                nonzero(integer, u64::MAX),
            ),
        ),
        None => {
            let exact_zero = nonzero(integer, u64::MAX) == Some(false)
                && original_guard == Some(false)
                && original_sticky == Some(false);
            if exact_zero {
                return Bits::constant(0, u64::MAX);
            }
            let inexact = or(original_guard, original_sticky).filter(|&value| value);
            return match component {
                1 => boolean(inexact),
                2 if flag(NEAREST) == Some(false) => boolean(and(flag(AWAY), inexact)),
                _ => Bits::default(),
            };
        }
    };
    let inexact = or(guard, sticky);
    let nearest_increment = and(guard, or(sticky, nonzero(floor, 1)));
    let directed = and(flag(AWAY), inexact);
    let incremented = match flag(NEAREST) {
        Some(true) => nearest_increment,
        Some(false) => directed,
        None if nearest_increment == directed => directed,
        None => None,
    };
    match component {
        0 => match incremented {
            Some(false) => floor,
            Some(true) => {
                let known_low = (floor.zero | floor.one).trailing_ones();
                Bits::constant(
                    floor.one.wrapping_add(1),
                    u64::MAX.checked_shr(64 - known_low).unwrap_or(0),
                )
            }
            None => {
                let max = !floor.zero;
                if let Some(max) = max.checked_add(1) {
                    let width = 64 - max.leading_zeros();
                    Bits {
                        zero: u64::MAX.checked_shl(width).unwrap_or(0),
                        one: 0,
                    }
                } else {
                    Bits::default()
                }
            }
        },
        1 => boolean(inexact),
        2 => boolean(incremented),
        _ => unreachable!("rounding has three results"),
    }
}

fn nonzero(bits: Bits, mask: u64) -> Option<bool> {
    if bits.one & mask != 0 {
        Some(true)
    } else if bits.zero & mask == mask {
        Some(false)
    } else {
        None
    }
}

fn boolean(value: Option<bool>) -> Bits {
    value
        .map(|value| Bits::constant(u64::from(value), u64::from(u32::MAX)))
        .unwrap_or_default()
}

fn and(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), value) | (value, Some(true)) => value,
        _ => None,
    }
}

fn or(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), value) | (value, Some(false)) => value,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_rounding_facts_agree_with_rational_arithmetic() {
        for integer in [0, 3, 1 << 63, u64::MAX] {
            for distance in [0_u32, 1, 32, 63, 64, 65, u32::MAX] {
                for control in 0..16_u64 {
                    let fraction = ((control & 1) << 1) | ((control >> 1) & 1);
                    let numerator = u128::from(integer) * 4 + u128::from(fraction);
                    let denominator = 1_u128 << distance.saturating_add(2).min(127);
                    let floor = numerator / denominator;
                    let remainder = numerator % denominator;
                    let increment = if control & u64::from(NEAREST) != 0 {
                        remainder * 2 > denominator
                            || (remainder * 2 == denominator && floor & 1 != 0)
                    } else {
                        control & u64::from(AWAY) != 0 && remainder != 0
                    };
                    let expected = [
                        (floor + u128::from(increment)) as u64,
                        u64::from(remainder != 0),
                        u64::from(increment),
                    ];
                    for integer_mask in [0, 1, 0xffff, !(1 << 63), u64::MAX] {
                        for distance_mask in [0, u64::from(u32::MAX)] {
                            for control_mask in [0, 3, 7, 15] {
                                for (component, expected) in expected.into_iter().enumerate() {
                                    let facts = infer(
                                        Bits::constant(integer, integer_mask),
                                        Bits::constant(u64::from(distance), distance_mask),
                                        Bits::constant(control, control_mask),
                                        component,
                                    );
                                    assert_eq!(expected & facts.zero, 0);
                                    assert_eq!(expected & facts.one, facts.one);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
