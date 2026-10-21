#![cfg(target_arch = "wasm32")]

use js_sys::{Array, ArrayBuffer, Uint8Array};
use relocade_worker::Message;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Debug, PartialEq, Message)]
struct Record<T>
where
    T: PartialEq,
{
    fields: T,
    value: Vec<u8>,
    tag: u32,
}

#[derive(Debug, PartialEq, Message)]
struct Pair<T>(T, Vec<u8>);

#[derive(Debug, PartialEq, Message)]
struct Empty;

#[derive(Debug, PartialEq, Message)]
enum Command<T: PartialEq> {
    Stop,
    Data(Pair<T>),
    Update { record: Record<T> },
}

trait Family {
    type Item;
}

struct Bytes;
impl Family for Bytes {
    type Item = Vec<u8>;
}

#[derive(Message)]
struct Associated<F: Family> {
    value: F::Item,
}

#[wasm_bindgen_test]
fn derived_records_preserve_fields_and_only_require_field_types_to_be_messages() {
    let input = Array::of3(&7.into(), &Uint8Array::from(&[42u8][..]), &u32::MAX.into());
    let record = Record::<u32>::decode(input.into()).unwrap();
    assert_eq!(
        record,
        Record {
            fields: 7,
            value: vec![42],
            tag: u32::MAX
        }
    );
    let encoded = Array::from(&record.encode().value);
    assert_eq!(encoded.length(), 3);
    assert_eq!(encoded.get(0).as_f64(), Some(7.0));
    assert_eq!(Uint8Array::new(&encoded.get(1)).to_vec(), [42]);
    assert_eq!(encoded.get(2).as_f64(), Some(f64::from(u32::MAX)));

    let pair = Pair("value".to_owned(), vec![9]);
    assert_eq!(
        Pair::<String>::decode(pair.encode().value).unwrap(),
        Pair("value".to_owned(), vec![9])
    );
    assert_eq!(Empty::decode(Empty.encode().value).unwrap(), Empty);

    // Bytes is not a Message; only its associated field type needs a codec.
    let associated = Associated::<Bytes> { value: vec![1, 2] };
    assert_eq!(
        Associated::<Bytes>::decode(associated.encode().value)
            .unwrap()
            .value,
        [1, 2]
    );
}

#[wasm_bindgen_test]
fn enum_variants_preserve_their_fields() {
    let stop = Command::<String>::decode(Command::<String>::Stop.encode().value).unwrap();
    assert_eq!(stop, Command::Stop);
    let data = Command::Data(Pair("value".to_owned(), vec![3, 4]));
    assert_eq!(
        Command::<String>::decode(data.encode().value).unwrap(),
        Command::Data(Pair("value".to_owned(), vec![3, 4]))
    );
    let update = Command::Update {
        record: Record {
            fields: "updated".to_owned(),
            value: vec![5],
            tag: 6,
        },
    };
    assert_eq!(
        Command::<String>::decode(update.encode().value).unwrap(),
        Command::Update {
            record: Record {
                fields: "updated".to_owned(),
                value: vec![5],
                tag: 6
            }
        }
    );
}

#[wasm_bindgen_test]
fn decoding_rejects_wrong_field_counts_types_and_variant_tags() {
    for count in [0, 2, 4] {
        assert!(Record::<u32>::decode(Array::new_with_length(count).into()).is_err());
    }
    assert!(
        Pair::<String>::decode(Array::of2(&42.into(), &Uint8Array::new_with_length(0)).into())
            .is_err()
    );
    for tag in [-1.0, 0.5, 3.0, 4_294_967_296.0, f64::NAN, f64::INFINITY] {
        assert!(Command::<String>::decode(Array::of2(&tag.into(), &Array::new()).into()).is_err());
    }
    // Stop has no fields; a valid tag must still validate its record shape.
    assert!(
        Command::<String>::decode(Array::of2(&0.into(), &Array::of1(&42.into())).into()).is_err()
    );
}

#[wasm_bindgen(inline_js = "
export function clone_message(value, transfer) { return structuredClone(value, {transfer}); }
")]
extern "C" {
    fn clone_message(value: &JsValue, transfer: &Array) -> JsValue;
}

#[wasm_bindgen_test]
fn derived_messages_transfer_buffers_from_nested_fields() {
    let command = Command::Update {
        record: Record {
            fields: vec![1u8, 2],
            value: vec![42],
            tag: 7,
        },
    };
    let packet = command.encode();
    assert_eq!(packet.transfer.len(), 2);
    let received = clone_message(&packet.value, &packet.transfer.iter().collect());
    for buffer in packet.transfer {
        assert_eq!(buffer.unchecked_into::<ArrayBuffer>().byte_length(), 0);
    }
    assert_eq!(
        Command::<Vec<u8>>::decode(received).unwrap(),
        Command::Update {
            record: Record {
                fields: vec![1, 2],
                value: vec![42],
                tag: 7
            }
        }
    );
}
