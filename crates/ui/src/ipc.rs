//! Typed access to the backend through `window.__TAURI__` (`app.withGlobalTauri`), using the
//! command and event definitions in `solidbar_shared::ipc`.

use serde::Serialize;
use solidbar_shared::ipc::{Command, CommandError, Event};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn tauri_listen(
        event: &str,
        handler: &Closure<dyn FnMut(JsValue)>,
    ) -> Result<JsValue, JsValue>;
}

/// Invokes a command; its argument travels as `{ "args": … }`.
pub async fn call<C: Command>(args: &C::Args) -> Result<C::Output, CommandError> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    let payload = js_sys::Object::new();
    let args = args
        .serialize(&serializer)
        .map_err(|error| CommandError::new(error.to_string()))?;
    js_sys::Reflect::set(&payload, &"args".into(), &args)
        .map_err(|_| CommandError::new("Couldn't prepare the request"))?;
    match tauri_invoke(C::NAME, payload.into()).await {
        Ok(value) => serde_wasm_bindgen::from_value(value).map_err(|error| {
            CommandError::new(format!("Unexpected reply to {}: {error}", C::NAME))
        }),
        Err(error) => Err(
            serde_wasm_bindgen::from_value(error.clone()).unwrap_or_else(|_| {
                CommandError::new(error.as_string().unwrap_or_else(|| format!("{error:?}")))
            }),
        ),
    }
}

/// Subscribes to an event for the lifetime of the page.
pub fn on<E: Event + 'static>(mut handler: impl FnMut(E::Payload) + 'static) {
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |message: JsValue| {
        let payload = js_sys::Reflect::get(&message, &"payload".into()).unwrap_or(JsValue::NULL);
        match serde_wasm_bindgen::from_value(payload) {
            Ok(payload) => handler(payload),
            Err(error) => web_sys::console::error_2(&E::NAME.into(), &error.to_string().into()),
        }
    });
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(error) = tauri_listen(E::NAME, &closure).await {
            web_sys::console::error_2(&E::NAME.into(), &error);
        }
        // The listener stays registered until the page unloads.
        closure.forget();
    });
}
