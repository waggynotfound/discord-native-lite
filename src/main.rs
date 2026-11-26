#![windows_subsystem = "windows"]
mod discord;
mod readings;
mod ui;
fn main() {
    if let Err(e) = ui::run() {
        ui::alert(&e);
    }
}
