//! Relocade's opaque snapshot for INT 33h save/restore.
//!
//! Fields use little-endian bytes in the order below. Runtime timing and the
//! background beneath a drawn cursor are rebuilt when the snapshot is restored.
use super::{
    cursor::{graphics, text, Cursor},
    motion::{Acceleration, Axis},
    state::{Callback, Counter, Point, State},
};
use relocade_machine::FarPointer;
use std::io::{Read, Write};

pub(super) const BYTE_LEN: usize = 172;

pub(super) fn encode(state: &State, cursor: &Cursor) -> [u8; BYTE_LEN] {
    let mut bytes = [0; BYTE_LEN];
    let mut output = bytes.as_mut_slice();
    let mut write = |field: &[u8]| {
        output.write_all(field).expect("fixed-size mouse snapshot");
    };
    for axis in [&state.x, &state.y] {
        for value in [axis.position, axis.bounds[0], axis.bounds[1]] {
            write(&value.to_le_bytes());
        }
        write(&axis.ratio.to_le_bytes());
        write(&axis.sensitivity.to_le_bytes());
        write(&axis.remainder.to_le_bytes());
    }
    write(&state.acceleration.threshold.to_le_bytes());
    write(&state.buttons.to_le_bytes());
    for value in state.motion {
        write(&value.to_le_bytes());
    }
    for counter in state.pressed.iter().chain(&state.released) {
        for value in [counter.count, counter.position.x, counter.position.y] {
            write(&value.to_le_bytes());
        }
    }
    write(&state.callback.mask.to_le_bytes());
    write(&state.callback.target.segment.to_le_bytes());
    write(&state.callback.target.offset.to_le_bytes());
    write(&cursor.visibility.to_le_bytes());
    write(&cursor.text.screen.to_le_bytes());
    write(&cursor.text.cursor.to_le_bytes());
    for value in cursor.graphics.hotspot {
        write(&value.to_le_bytes());
    }
    for value in cursor.graphics.screen.iter().chain(&cursor.graphics.cursor) {
        write(&value.to_le_bytes());
    }
    assert!(output.is_empty());
    bytes
}

pub(super) fn decode(bytes: &[u8; BYTE_LEN]) -> Option<(State, Cursor)> {
    let mut input = bytes.as_slice();
    let x = read_axis(&mut input)?;
    let y = read_axis(&mut input)?;
    let mut word = || u16::from_le_bytes(read(&mut input));
    let threshold = word();
    let buttons = word();
    if threshold == 0 || buttons & !7 != 0 {
        return None;
    }
    let motion = [word() as i16, word() as i16];
    let mut counters = || {
        std::array::from_fn(|_| Counter {
            count: word(),
            position: Point {
                x: word(),
                y: word(),
            },
        })
    };
    let pressed = counters();
    let released = counters();
    let callback = Callback {
        mask: word(),
        target: FarPointer {
            segment: word(),
            offset: word(),
        },
    };
    let visibility = word() as i16;
    if visibility > 0 {
        return None;
    }
    let text = text::Shape {
        screen: word(),
        cursor: word(),
    };
    let graphics = graphics::Shape {
        hotspot: [word() as i16, word() as i16],
        screen: std::array::from_fn(|_| word()),
        cursor: std::array::from_fn(|_| word()),
    };
    assert!(input.is_empty());
    let cursor = Cursor::from_state(visibility, text, graphics);
    let mut acceleration = Acceleration::default();
    acceleration.set_threshold(threshold);
    Some((
        State {
            x,
            y,
            acceleration,
            buttons,
            pressed,
            released,
            motion,
            callback,
        },
        cursor,
    ))
}

fn read_axis(input: &mut &[u8]) -> Option<Axis> {
    let position = i32::from_le_bytes(read(input));
    let bounds = std::array::from_fn(|_| i32::from_le_bytes(read(input)));
    let ratio = u16::from_le_bytes(read(input));
    let sensitivity = u16::from_le_bytes(read(input));
    let remainder = i64::from_le_bytes(read(input));
    if bounds[0] < i32::from(i16::MIN)
        || bounds[1] > i32::from(u16::MAX)
        || bounds[0] > bounds[1]
        || !(bounds[0]..=bounds[1]).contains(&position)
        || ratio == 0
        || !(1..=100).contains(&sensitivity)
        || remainder.unsigned_abs() >= u64::from(ratio) * 32 * 5
    {
        return None;
    }
    Some(Axis {
        position,
        bounds,
        ratio,
        sensitivity,
        remainder,
    })
}

fn read<const N: usize>(input: &mut &[u8]) -> [u8; N] {
    let mut bytes = [0; N];
    input
        .read_exact(&mut bytes)
        .expect("fixed-size mouse snapshot");
    bytes
}
