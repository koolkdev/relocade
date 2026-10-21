use crate::{Error, Message, Result};
use js_sys::{Array, Uint8Array, WebAssembly};
use wasm_bindgen::{JsCast, JsValue};

/// A structured-clone value and the owned buffers or ports to transfer with it.
/// Encoders must never include live Wasm-memory buffers in the transfer list.
pub struct Packet {
    pub value: JsValue,
    pub transfer: Vec<JsValue>,
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
        let mut first = self.0.encode();
        let second = self.1.encode();
        first.value = Array::of2(&first.value, &second.value).into();
        first.transfer.extend(second.transfer);
        first
    }

    fn decode(value: JsValue) -> Result<Self> {
        let values = value.dyn_into::<Array>().map_err(invalid_message)?;
        Ok((A::decode(values.get(0))?, B::decode(values.get(1))?))
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

impl Message for Error {
    fn encode(self) -> Packet {
        match self {
            Error::Closed => JsValue::NULL,
            Error::Failed(message) => message.into(),
        }
        .encode()
    }

    fn decode(value: JsValue) -> Result<Self> {
        if value.is_null() {
            return Ok(Error::Closed);
        }
        value
            .as_string()
            .map(Error::Failed)
            .ok_or_else(|| invalid_message(value))
    }
}

fn tagged(tag: bool, mut packet: Packet) -> Packet {
    packet.value = Array::of2(&tag.into(), &packet.value).into();
    packet
}

fn untag(value: JsValue) -> Result<(bool, JsValue)> {
    let values = value.dyn_into::<Array>().map_err(invalid_message)?;
    let tag = values.get(0);
    Ok((
        tag.as_bool().ok_or_else(|| invalid_message(tag))?,
        values.get(1),
    ))
}

fn invalid_message(_: JsValue) -> Error {
    Error::Failed("invalid typed worker message".into())
}
