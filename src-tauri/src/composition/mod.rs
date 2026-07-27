//! Composition root: constructs the concrete modules, process-lifetime shared state,
//! background execution resources, and the Tauri application. Framework types stay here
//! and in `transport`; application modules never import Tauri.

/// Scaffold command retained so the scaffold React page keeps working. The Phase 3
/// frontend bootstrap deletes it together with that page.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
