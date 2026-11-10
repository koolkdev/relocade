/// Composes posted values without requiring their Rust types to implement serde.
/// Constructed by the Transfer derive; each leaf owns its browser representation.
#[doc(hidden)]
pub struct PayloadPair<A, B>(pub A, pub B);

#[cfg(target_arch = "wasm32")]
mod web {
    use super::PayloadPair;
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
}
