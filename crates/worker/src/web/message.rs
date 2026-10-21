use crate::{Error, Message, Result};
use js_sys::{Array, Uint8Array, WebAssembly};
use wasm_bindgen::{JsCast, JsValue};

/// A structured-clone value and the owned buffers or ports to transfer with it.
/// Encoders must never include live Wasm-memory buffers in the transfer list.
pub struct Packet {
    pub value: JsValue,
    pub transfer: Vec<JsValue>,
}

/// Field composition used by the derive and built-in aggregate messages.
pub fn encode_fields<const N: usize>(fields: [Packet; N]) -> Packet {
    let values = Array::new();
    let mut transfer = Vec::new();
    for field in fields {
        values.push(&field.value);
        transfer.extend(field.transfer);
    }
    Packet {
        value: values.into(),
        transfer,
    }
}

pub fn decode_fields<const N: usize>(value: JsValue) -> Result<[JsValue; N]> {
    let fields = value.dyn_into::<Array>().map_err(invalid_message)?;
    if fields.length() as usize != N {
        return Err(invalid_message(fields.into()));
    }
    Ok(std::array::from_fn(|index| fields.get(index as u32)))
}

impl Message for JsValue {
    fn encode(self) -> Packet {
        Packet {
            value: self,
            transfer: vec![],
        }
    }
    fn decode(value: JsValue) -> Result<Self> {
        Ok(value)
    }
}

impl Message for String {
    fn encode(self) -> Packet {
        JsValue::from(self).encode()
    }

    fn decode(value: JsValue) -> Result<Self> {
        value.as_string().ok_or_else(|| invalid_message(value))
    }
}

impl Message for u32 {
    fn encode(self) -> Packet {
        JsValue::from(self).encode()
    }

    fn decode(value: JsValue) -> Result<Self> {
        let number = value.as_f64().ok_or_else(|| invalid_message(value))?;
        if number.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&number) {
            return Err(invalid_message(number.into()));
        }
        Ok(number as u32)
    }
}

impl Message for Vec<u8> {
    fn encode(self) -> Packet {
        // Copy out of Wasm memory into an owned buffer that can be transferred.
        let bytes = Uint8Array::from(self.as_slice());
        Packet {
            transfer: vec![bytes.buffer().into()],
            value: bytes.into(),
        }
    }

    fn decode(value: JsValue) -> Result<Self> {
        Ok(value
            .dyn_into::<Uint8Array>()
            .map_err(invalid_message)?
            .to_vec())
    }
}

impl Message for WebAssembly::Module {
    fn encode(self) -> Packet {
        JsValue::from(self).encode()
    }
    fn decode(value: JsValue) -> Result<Self> {
        value.dyn_into().map_err(invalid_message)
    }
}

impl<A: Message, B: Message> Message for (A, B) {
    fn encode(self) -> Packet {
        encode_fields([self.0.encode(), self.1.encode()])
    }

    fn decode(value: JsValue) -> Result<Self> {
        let [first, second] = decode_fields(value)?;
        Ok((A::decode(first)?, B::decode(second)?))
    }
}

impl<T: Message> Message for Option<T> {
    fn encode(self) -> Packet {
        match self {
            Some(value) => tagged(true, value.encode()),
            None => tagged(false, JsValue::NULL.encode()),
        }
    }

    fn decode(value: JsValue) -> Result<Self> {
        let (present, value) = untag(value)?;
        if present {
            T::decode(value).map(Some)
        } else {
            Ok(None)
        }
    }
}

impl<T: Message> Message for Result<T> {
    fn encode(self) -> Packet {
        match self {
            Ok(value) => tagged(true, value.encode()),
            Err(error) => tagged(false, error.encode()),
        }
    }

    fn decode(value: JsValue) -> Result<Self> {
        let (success, value) = untag(value)?;
        if success {
            T::decode(value).map(Ok)
        } else {
            Error::decode(value).map(Err)
        }
    }
}

fn tagged(tag: bool, mut packet: Packet) -> Packet {
    packet.value = Array::of2(&tag.into(), &packet.value).into();
    packet
}

fn untag(value: JsValue) -> Result<(bool, JsValue)> {
    let [tag, value] = decode_fields(value)?;
    Ok((tag.as_bool().ok_or_else(|| invalid_message(tag))?, value))
}

fn invalid_message(_: JsValue) -> Error {
    Error::Failed("invalid typed worker message".into())
}
