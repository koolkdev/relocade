#![cfg(not(target_arch = "wasm32"))]

use relocade_wasm_bindings::{lift_flat, lower_flat, Abi, AbiType, AbiValue};

#[derive(Abi, Debug, PartialEq)]
struct Header<T = u16> {
    code: i8,
    value: T,
    enabled: bool,
}

#[derive(Abi, Debug, PartialEq)]
struct Packet {
    header: Header,
    weight: f64,
}

#[derive(Abi, Debug, PartialEq)]
struct Batch<T, const N: usize> {
    items: [T; N],
}

#[derive(Abi, Debug, PartialEq)]
struct Pair(u16, u64);

#[derive(Abi, Debug, PartialEq)]
struct Empty;

#[test]
fn structs_flatten_nested_fields_in_declaration_order() {
    let packet = Packet {
        header: Header {
            code: -1,
            value: 0x8001,
            enabled: true,
        },
        weight: -0.0,
    };
    let flat = lower_flat::<Packet>(packet);
    assert_eq!(
        (flat.0, flat.1, flat.2, flat.3.to_bits()),
        (255, 0x8001, 1, 1 << 63)
    );
    let packet = lift_flat::<Packet>(flat).unwrap();
    assert_eq!(
        packet.header,
        Header {
            code: -1,
            value: 0x8001,
            enabled: true
        }
    );
    assert_eq!(packet.weight.to_bits(), 1 << 63);
    assert_eq!(lower_flat::<Pair>(Pair(u16::MAX, u64::MAX)), (65535, -1));
    assert_eq!(lift_flat::<Empty>(()), Some(Empty));
}

#[test]
fn arrays_preserve_empty_shapes_and_narrow_signed_bits() {
    assert_eq!(Batch::<u8, 0>::lift(()), Some(Batch { items: [] }));
    type Pairs = Batch<(i8, bool), 2>;
    assert_eq!(
        lower_flat::<Pairs>(Batch {
            items: [(-1, true), (i8::MIN, false)]
        }),
        (255, 1, 128, 0)
    );
    assert_eq!(
        lift_flat::<Pairs>((255, 1, 128, 0)),
        Some(Batch {
            items: [(-1, true), (i8::MIN, false)]
        })
    );
}

#[derive(Abi, Debug, PartialEq)]
#[repr(u8)]
enum Reply {
    Empty = 37,
    Byte(i8) = 61,
    Float(f32) = 84,
    Wide(u64) = 97,
    Double(f64) = 103,
    Pair { code: u16, enabled: bool } = 119,
}

#[test]
fn enum_tags_follow_declaration_order_and_payloads_overlap() {
    assert_eq!(lower_flat::<Reply>(Reply::Empty), (0, 0_i64, 0));
    assert_eq!(lower_flat::<Reply>(Reply::Byte(-1)), (1, 255_i64, 0));
    assert_eq!(lower_flat::<Reply>(Reply::Wide(u64::MAX)), (3, -1_i64, 0));
    assert_eq!(
        lower_flat::<Reply>(Reply::Pair {
            code: 0x8001,
            enabled: true
        }),
        (5, 0x8001_i64, 1)
    );
    for bits in [0xffc1_2345, 0x8000_0000, 1] {
        let flat = lower_flat::<Reply>(Reply::Float(f32::from_bits(bits)));
        assert_eq!(flat, (2, i64::from(bits), 0));
        let Some(Reply::Float(value)) = lift_flat::<Reply>(flat) else {
            panic!("float variant")
        };
        assert_eq!(value.to_bits(), bits);
    }
    for bits in [0xfff8_1234_5678_9abc, 1 << 63, 1] {
        let flat = lower_flat::<Reply>(Reply::Double(f64::from_bits(bits)));
        assert_eq!(flat, (4, bits as i64, 0));
        let Some(Reply::Double(value)) = lift_flat::<Reply>(flat) else {
            panic!("double variant")
        };
        assert_eq!(value.to_bits(), bits);
    }
}

#[derive(Abi)]
enum Small {
    Byte(u8),
    Float(f32),
}

#[derive(Abi)]
enum FloatOnly {
    Empty,
    Float(f32),
}

#[test]
fn float_payloads_use_the_smallest_shared_carrier() {
    assert_eq!(lower_flat::<Small>(Small::Byte(255)), (0, 255_i32));
    let flat = lower_flat::<Small>(Small::Float(f32::from_bits(0xffc1_2345)));
    assert_eq!(flat, (1, 0xffc1_2345_u32 as i32));
    let Some(Small::Float(value)) = lift_flat::<Small>(flat) else {
        panic!("float variant")
    };
    assert_eq!(value.to_bits(), 0xffc1_2345);
    assert_eq!(lower_flat::<FloatOnly>(FloatOnly::Empty), (0, 0.0_f32));
    let flat = lower_flat::<FloatOnly>(FloatOnly::Float(-0.0));
    assert_eq!((flat.0, flat.1.to_bits()), (1, 0x8000_0000));
}

#[test]
fn invalid_tags_and_active_boolean_slots_fail_conversion() {
    assert!(lift_flat::<Reply>((99, 0, 0)).is_none());
    assert!(lift_flat::<Reply>((5, 7, 2)).is_none());
    assert_eq!(lift_flat::<Reply>((0, -1, -1)), Some(Reply::Empty));
    assert!(lift_flat::<bool>((-1,)).is_none());
}

#[test]
fn logical_shapes_do_not_require_rust_codecs() {
    struct CompilerOnly;
    impl AbiType for CompilerOnly {
        type Logical = Self;
    }
    fn shape<T: AbiType>() {}
    shape::<Header<CompilerOnly>>();
    shape::<Batch<CompilerOnly, 16>>();
}
