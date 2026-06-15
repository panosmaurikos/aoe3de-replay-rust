// Prevents an extra console window on Windows in release, do not remove!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    aoe3de_replay_desktop_lib::run()
}
