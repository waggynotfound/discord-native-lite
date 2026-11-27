//! Disposable receiver for release testing, not shipped in the application.
use std::{
    env, fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn main() {
    let exe = env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let args: Vec<_> = env::args().skip(1).collect();
    let child = args.iter().any(|s| s == "--type=renderer");
    let mut renderer = None;
    if !child {
        fs::write(dir.join("receipt"), args.join("\n")).unwrap();
        renderer = Some(
            Command::new(&exe)
                .args(args)
                .arg("--type=renderer")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(90) && !dir.join("finish").exists() {
        thread::sleep(Duration::from_millis(80))
    }
    if let Some(mut p) = renderer {
        let _ = p.wait();
    }
}
