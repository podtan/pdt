//! Compile-time OpenAPI spec generator.
//!
//! Usage: cargo run --bin generate-openapi -- > docs/openapi/pdt-openapi.json
//!
//! No database or network required — the spec is derived entirely from
//! the utoipa annotations in the source code.

use pdt::openapi::ApiDoc;
use utoipa::OpenApi;

fn main() {
    let spec = ApiDoc::openapi();
    let json = serde_json::to_string_pretty(&spec).expect("Failed to serialize OpenAPI spec");
    println!("{}", json);
}
