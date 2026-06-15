use aoe3de_replay_rust::{parse_all_with_options, ParseOptions};
use serde_json::Value;
use std::path::Path;

/// Parse a `.age3Yrec` file into the viewer's JSON shape (same output as the
/// CLI's `parse` with `--events`, the default honesty level: verified gameplay
/// events plus playerStates, no experimental shipments). Returned to the
/// webview, which feeds it straight into `loadData`.
#[tauri::command]
fn parse_replay(path: String) -> Result<Value, String> {
    let bytes = std::fs::read(Path::new(&path))
        .map_err(|err| format!("Could not read replay '{path}': {err}"))?;
    let options = ParseOptions {
        debug_commands: false,
        experimental_shipments: false,
        events: true,
    };
    let parsed = parse_all_with_options(&bytes, options)?;
    serde_json::to_value(&parsed).map_err(|err| format!("Could not serialize result: {err}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![parse_replay])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
