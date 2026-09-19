//! WebAssembly bindings for the XLAIR SUS SVG renderer.

use wasm_bindgen::prelude::*;

/// Parses an XLAIR-compatible SUS document and returns its SVG preview.
#[wasm_bindgen]
pub fn render_sus_svg(source: &str) -> Result<String, JsValue> {
    xlair_sus_svg::render(source).map_err(|error| JsValue::from_str(&error.to_string()))
}
