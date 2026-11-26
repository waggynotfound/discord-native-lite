//! Opt-in: exercises the real compiled GUI, Windows process counters and launch handoff.
//! LOCALAPPDATA is overridden only in the test child. No real Discord is launched.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    UI::{Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::*},
};
unsafe extern "system" fn find(h: HWND, p: isize) -> i32 {
    let ctx = &mut *(p as *mut (u32, HWND));
    let mut pid = 0;
    GetWindowThreadProcessId(h, &mut pid);
    let mut cls = [0u16; 64];
    let n = GetClassNameW(h, cls.as_mut_ptr(), cls.len() as i32);
    if ctx.0 == pid && String::from_utf16_lossy(&cls[..n as usize]) == "RelayNativeRust" {
        ctx.1 = h;
        0
    } else {
        1
    }
}
fn wait<T>(msg: &str, sec: u64, mut call: impl FnMut() -> Option<T>) -> T {
    let at = Instant::now();
    loop {
        if let Some(v) = call() {
            return v;
        }
        assert!(at.elapsed() < Duration::from_secs(sec), "{msg}");
        thread::sleep(Duration::from_millis(80));
    }
}
struct Gui {
    p: Child,
    h: HWND,
}
impl Gui {
    fn open(local: &Path) -> Self {
        let mut p = Command::new(env!("CARGO_BIN_EXE_Relay"))
            .env("LOCALAPPDATA", local)
            .spawn()
            .unwrap();
        let h = wait("Fixture GUI did not open", 10, || {
            assert!(
                p.try_wait().unwrap().is_none(),
                "Relay exited before opening its GUI"
            );
            let mut ctx: (u32, HWND) = (p.id(), std::ptr::null_mut());
            unsafe {
                EnumWindows(Some(find), &mut ctx as *mut _ as isize);
            };
            (!ctx.1.is_null()).then_some(ctx.1)
        });
        unsafe {
            ShowWindow(h, SW_RESTORE);
        }
        Self { p, h }
    }
    fn text(&self, id: i32) -> String {
        unsafe {
            // Automation can minimize new top-level windows. Visible-screen checks
            // must not wait for a refresh that Relay deliberately pauses when minimized.
            if IsIconic(self.h) != 0 {
                ShowWindow(self.h, SW_RESTORE);
            }
            let h = GetDlgItem(self.h, id);
            let mut b = [0u16; 512];
            let n = SendMessageW(h, WM_GETTEXT, b.len(), b.as_mut_ptr() as isize);
            String::from_utf16_lossy(&b[..n as usize])
        }
    }
    fn click(&self, id: i32) {
        unsafe {
            let h = GetDlgItem(self.h, id);
            assert_ne!(IsWindowEnabled(h), 0, "Control {id} is disabled");
            SendMessageW(h, BM_CLICK, 0, 0);
        }
    }
}
impl Drop for Gui {
    fn drop(&mut self) {
        if self.p.try_wait().ok().flatten().is_none() {
            unsafe {
                PostMessageW(self.h, WM_CLOSE, 0, 0);
            }
            let _ = self.p.wait();
        }
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let d = self.0.join("Discord/app-1.0.123");
        let _ = fs::write(d.join("finish.fixture"), b"quit");
        thread::sleep(Duration::from_millis(450));
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "opens a disposable native Windows GUI and records 30 seconds"]
fn full_native_flow() {
    let tmp = Temp(std::env::temp_dir().join(format!("Relay UI fixture {}", std::process::id())));
    let d = tmp.0.join("Discord/app-1.0.123");
    fs::create_dir_all(d.join("resources")).unwrap();
    fs::write(d.join("resources/app.asar"), b"Vencord test marker").unwrap();
    let meter = tmp.0.join("RelayLauncher/readings");
    fs::create_dir_all(&meter).unwrap();
    let baseline = meter.join("Stable-1.0.123-Standard.tsv");
    let old = "2026-11-26T09:38:31.0000000Z\t104857600\t209715200";
    fs::write(&baseline, old).unwrap();
    let out = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition=2021", "tests/fixture.rs", "-o"])
        .arg(d.join("Discord.exe"))
        .status()
        .unwrap();
    assert!(out.success());
    for (id, expected, flags) in [
        (202, "Stock session", ""),
        (204, "Balanced session", "--js-flags=--optimize-for-size"),
        (
            201,
            "Strong saver session",
            "--js-flags=--optimize-for-size --memory-saver-mode --heap-growing-percent=15",
        ),
    ] {
        let mut g = Gui::open(&tmp.0);
        wait("launch state", 8, || {
            (g.text(25) == "Ready to launch").then_some(())
        });
        assert_eq!(g.text(14), "Vencord detected");
        if id != 201 {
            g.click(id);
        } // Also verify the default is Strong saver.
        g.click(203);
        wait("launcher did not exit after handoff", 6, || {
            g.p.try_wait().unwrap()
        });
        drop(g);
        let args = wait("test receiver not started", 6, || {
            fs::read_to_string(d.join("arguments.txt")).ok()
        });
        assert_eq!(args, flags);
        let g = Gui::open(&tmp.0);
        wait("running guard", 8, || {
            (g.text(25) == "Discord is running").then_some(())
        });
        assert_eq!(unsafe { IsWindowEnabled(GetDlgItem(g.h, 203)) }, 0);
        g.click(102);
        wait("profile not verified in real renderer counters", 12, || {
            g.text(41).starts_with(expected).then_some(())
        });
        assert_eq!(g.text(36), "100 MiB");
        assert!(g.text(32).contains("2 processes"));
        assert!(g.text(32).contains("renderer"));
        if id == 201 {
            g.click(301);
            wait("recording not saved", 55, || {
                g.text(41).contains("reading saved").then_some(())
            });
            assert!(g.text(43).ends_with("MiB"));
            assert!(meter.join("Stable-1.0.123-Strong.tsv").is_file());
            assert!(!meter.join("Stable-1.0.123-Saver.tsv").is_file())
        }
        fs::write(d.join("finish.fixture"), b"done").unwrap();
        g.click(101);
        wait("fixture did not close", 16, || {
            (g.text(25) == "Ready to launch").then_some(())
        });
        drop(g);
        fs::remove_file(d.join("finish.fixture")).unwrap();
        fs::remove_file(d.join("arguments.txt")).unwrap();
    }
    assert_eq!(fs::read_to_string(baseline).unwrap(), old);
}
