//! Compiled by the native integration test, never shipped inside Relay.exe.
#![windows_subsystem = "windows"]
use std::{
    fs, thread,
    time::{Duration, Instant},
};
fn main() {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let is_child = a.iter().any(|v| v == "--type=renderer");
    let mut child = None;
    if !is_child {
        fs::write(dir.join("arguments.txt"), a.join("\n")).unwrap();
        child = Some(
            std::process::Command::new(&exe)
                .args(&a)
                .arg("--type=renderer")
                .spawn()
                .unwrap(),
        );
    }
    let start = Instant::now();
    while !dir.join("finish.fixture").exists() && start.elapsed() < Duration::from_secs(100) {
        thread::sleep(Duration::from_millis(100));
    }
    if let Some(mut p) = child {
        let _ = p.wait();
    }
}
