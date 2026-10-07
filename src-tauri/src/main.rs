#[cfg_attr(mobile, tauri::mobile_entry_point)]
fn main() {
    readflow_lib::run();
}
