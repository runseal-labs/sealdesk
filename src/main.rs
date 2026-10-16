#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use sealdesk::*;

fn main() {
    if !common::global_init() {
        eprintln!("Global initialization failed.");
        return;
    }
    common::global_clean();
}
