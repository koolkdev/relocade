use super::{PayloadOption, PayloadPair};
use crate::{AsJs, Post};
use js_sys::Array;
use wasm_bindgen::{JsCast, JsValue};

impl<A: Post, B: Post> AsJs for PayloadPair<A, B> {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        Ok(Array::of2(&self.0.to_js()?, &self.1.to_js()?).into())
    }

    fn from_js(value: JsValue) -> Result<Self, JsValue> {
        let values = value.dyn_into::<Array>()?;
        if values.length() != 2 {
            return Err(JsValue::from_str("expected two transfer payloads"));
        }
        Ok(Self(A::from_js(values.get(0))?, B::from_js(values.get(1))?))
    }
}

impl<A: Post, B: Post> Post for PayloadPair<A, B> {
    fn transferables(&self) -> Array {
        self.0.transferables().concat(&self.1.transferables())
    }
}

impl<T: Post> AsJs for PayloadOption<T> {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        let values = Array::new();
        if let Some(value) = &self.0 {
            values.push(&value.to_js()?);
        }
        Ok(values.into())
    }

    fn from_js(value: JsValue) -> Result<Self, JsValue> {
        let values = value.dyn_into::<Array>()?;
        match values.length() {
            0 => Ok(Self(None)),
            1 => Ok(Self(Some(T::from_js(values.get(0))?))),
            _ => Err(JsValue::from_str("expected zero or one transfer payload")),
        }
    }
}

impl<T: Post> Post for PayloadOption<T> {
    fn transferables(&self) -> Array {
        self.0.as_ref().map_or_else(Array::new, Post::transferables)
    }
}
