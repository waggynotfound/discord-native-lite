//! Win32 controls on the UI thread; only the meter uses a worker. No rendering loop.
use crate::{
    discord::{self, Client, Meter},
    readings::{self, Mode, Snapshot},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::OsStr,
    mem::zeroed,
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    ptr::{null, null_mut},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::GetDpiForSystem, Input::KeyboardAndMouse::EnableWindow, WindowsAndMessaging::*},
};
const BLUE: u32 = 0x8a3e2b;
const PAGE: u32 = 0xfdfaf8;
const INK: u32 = 0x402518;
const FADE: u32 = 0x725c4c;
const ICE: u32 = 0xffdeb4;
const LAUNCH: i32 = 101;
const COMPARE: i32 = 102;
const CHANNEL: i32 = 103;
const SAVER: i32 = 201;
const STOCK: i32 = 202;
const BALANCED: i32 = 204;
const START: i32 = 203;
const RECORD: i32 = 301;
const PING: u32 = WM_APP + 1;
const RESULT: u32 = WM_APP + 2;
fn w(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}
fn xerr() -> String {
    std::io::Error::last_os_error().to_string()
}
pub fn alert(s: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            w(s).as_ptr(),
            w("Relay").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
thread_local! {static APP:RefCell<Option<App>>=const{RefCell::new(None)};}
fn access(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Ok(mut slot) = a.try_borrow_mut() {
            if let Some(s) = slot.as_mut() {
                f(s)
            }
        }
    })
}
struct Widget {
    h: HWND,
    page: u8,
    txt: String,
}
struct Job {
    seq: u64,
    client: Client,
}
struct Worker {
    tx: Sender<Job>,
    rx: Receiver<(u64, Result<Snapshot, String>)>,
}
impl Worker {
    fn new(h: HWND) -> Self {
        let (tx, q) = mpsc::channel::<Job>();
        let (send, rx) = mpsc::channel();
        let hwnd = h as usize;
        thread::spawn(move || {
            let mut meter = None;
            for j in q {
                let answer = (|| {
                    if meter.is_none() {
                        meter = Some(Meter::new()?)
                    }
                    meter.as_ref().unwrap().sample(&j.client)
                })();
                if answer.is_err() {
                    meter = None
                }
                if send.send((j.seq, answer)).is_err() {
                    break;
                }
                unsafe {
                    PostMessageW(hwnd as HWND, RESULT, 0, 0);
                }
            }
        });
        Self { tx, rx }
    }
}
struct App {
    h: HWND,
    dpi: i32,
    widgets: HashMap<i32, Widget>,
    fonts: Vec<HFONT>,
    clients: Vec<Client>,
    idx: usize,
    home: PathBuf,
    page: u8,
    installed: Option<String>,
    live: Option<Snapshot>,
    worker: Option<Worker>,
    seq: u64,
    pending: bool,
    recording: Option<(Snapshot, Vec<Snapshot>)>,
    due: Instant,
    notice: Option<String>,
}
impl Drop for App {
    fn drop(&mut self) {
        for f in self.fonts.drain(..) {
            unsafe {
                DeleteObject(f);
            }
        }
    }
}
impl App {
    fn sc(&self, n: i32) -> i32 {
        (n * self.dpi + 48) / 96
    }
    fn c(&self) -> &Client {
        &self.clients[self.idx]
    }
    #[allow(clippy::too_many_arguments)]
    fn widget(
        &mut self,
        id: i32,
        page: u8,
        class: &str,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        style: u32,
        font: usize,
    ) -> Result<(), String> {
        unsafe {
            let h = CreateWindowExW(
                0,
                w(class).as_ptr(),
                w(text).as_ptr(),
                WS_CHILD
                    | style
                    | if page == 0 || page == self.page {
                        WS_VISIBLE
                    } else {
                        0
                    },
                self.sc(x),
                self.sc(y),
                self.sc(width),
                self.sc(height),
                self.h,
                id as HMENU,
                GetModuleHandleW(null()),
                null(),
            );
            if h.is_null() {
                return Err(xerr());
            }
            SendMessageW(h, WM_SETFONT, self.fonts[font] as usize, 0);
            self.widgets.insert(
                id,
                Widget {
                    h,
                    page,
                    txt: text.into(),
                },
            );
            Ok(())
        }
    }
    fn init(&mut self) -> Result<(), String> {
        unsafe {
            for (px, weight) in [(15, 400), (15, 600), (30, 600), (13, 400), (21, 600)] {
                let font = CreateFontW(
                    -self.sc(px),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    0,
                    w("Segoe UI").as_ptr(),
                );
                if font.is_null() {
                    return Err(xerr());
                }
                self.fonts.push(font)
            }
        }
        // Control ID, page (0 = shared), label, bounds in logical pixels, font index.
        let labels = [
            (10, 0, "Relay", 22, 31, 154, 45, 2),
            (11, 0, "Discord launcher", 23, 82, 160, 29, 0),
            (12, 0, "Discord channel", 21, 288, 165, 21, 3),
            (13, 0, "", 21, 360, 169, 22, 3),
            (14, 0, "", 21, 386, 169, 32, 3),
            (21, 1, "Launch Discord", 219, 24, 441, 46, 2),
            (22, 1, "Your desktop client, account and plugins.", 221, 79, 438, 31, 0),
            (23, 1, "Collect unused JavaScript memory sooner. May use more CPU.", 245, 146, 425, 43, 0),
            (27, 1, "Previous saver: favor smaller JavaScript code and heaps.", 245, 218, 425, 29, 3),
            (24, 1, "No memory flags.", 245, 275, 407, 22, 3),
            (25, 1, "", 221, 298, 444, 26, 1),
            (26, 1, "", 221, 332, 439, 57, 0),
            (31, 2, "Compare RAM", 219, 24, 443, 46, 2),
            (32, 2, "Checking the current session...", 221, 79, 438, 31, 0),
            (33, 2, "For comparable readings, use the same channel or call and wait a minute after each fresh launch.", 221, 120, 434, 61, 0),
            (34, 2, "Stock", 221, 200, 138, 22, 1),
            (35, 2, "Balanced", 374, 200, 138, 22, 1),
            (42, 2, "Strong saver", 527, 200, 142, 22, 1),
            (36, 2, "Not recorded", 221, 231, 148, 34, 4),
            (37, 2, "Not recorded", 374, 231, 148, 34, 4),
            (43, 2, "Not recorded", 527, 231, 160, 34, 4),
            (38, 2, "", 221, 269, 146, 41, 3),
            (39, 2, "", 374, 269, 146, 41, 3),
            (44, 2, "", 527, 269, 153, 41, 3),
            (40, 2, "", 221, 319, 444, 25, 1),
            (41, 2, "", 221, 351, 442, 42, 3),
        ];
        for (id, page, text, x, y, width, height, font) in labels {
            self.widget(id, page, "STATIC", text, x, y, width, height, 0, font)?;
        }
        for (id, p, t, x, y, ww, hh, style) in [
            (LAUNCH, 0, "Launch", 16, 142, 161, 35, BS_PUSHBUTTON),
            (COMPARE, 0, "Compare RAM", 16, 184, 161, 35, BS_PUSHBUTTON),
            (
                SAVER,
                1,
                "Strong saver",
                220,
                113,
                280,
                29,
                BS_AUTORADIOBUTTON,
            ),
            (
                BALANCED,
                1,
                "Balanced",
                220,
                186,
                280,
                29,
                BS_AUTORADIOBUTTON,
            ),
            (STOCK, 1, "Stock", 220, 245, 280, 29, BS_AUTORADIOBUTTON),
            (START, 1, "Start Discord", 220, 403, 180, 38, BS_PUSHBUTTON),
            (
                RECORD,
                2,
                "Record 30 seconds",
                220,
                403,
                180,
                38,
                BS_PUSHBUTTON,
            ),
        ] {
            self.widget(
                id,
                p,
                "BUTTON",
                t,
                x,
                y,
                ww,
                hh,
                style as u32
                    | WS_TABSTOP
                    | if id == SAVER || id == START {
                        WS_GROUP
                    } else {
                        0
                    },
                1,
            )?
        }
        self.widget(
            CHANNEL,
            0,
            "COMBOBOX",
            "",
            21,
            317,
            153,
            184,
            CBS_DROPDOWNLIST as u32 | WS_VSCROLL | WS_TABSTOP,
            0,
        )?;
        unsafe {
            for c in &self.clients {
                SendMessageW(
                    self.id(CHANNEL),
                    CB_ADDSTRING,
                    0,
                    w(c.name).as_ptr() as isize,
                );
            }
            SendMessageW(self.id(CHANNEL), CB_SETCURSEL, 0, 0);
            SendMessageW(self.id(SAVER), BM_SETCHECK, 1, 0);
        }
        self.select(0);
        Ok(())
    }
    fn id(&self, id: i32) -> HWND {
        self.widgets[&id].h
    }
    fn text(&mut self, id: i32, s: impl AsRef<str>) {
        let el = self.widgets.get_mut(&id).unwrap();
        let s = s.as_ref();
        if el.txt != s {
            unsafe {
                SetWindowTextW(el.h, w(s).as_ptr());
            }
            el.txt = s.into();
        }
    }
    fn enable(&self, id: i32, on: bool) {
        unsafe {
            EnableWindow(self.id(id), i32::from(on));
        }
    }
    fn clear_worker(&mut self) {
        self.seq = self.seq.wrapping_add(1);
        self.worker = None;
        self.pending = false;
        self.live = None;
    }
    fn select(&mut self, i: usize) {
        if i >= self.clients.len() {
            return;
        }
        self.idx = i;
        self.clear_worker();
        self.notice = None;
        let install = self.c().latest();
        let mut mark = false;
        self.installed = install.ok().flatten().map(|(v, p)| {
            mark = discord::vencord(&p);
            v
        });
        self.text(
            13,
            self.installed
                .as_ref()
                .map(|v| format!("Installed: {v}"))
                .unwrap_or_else(|| "No installed build".into()),
        );
        self.text(
            14,
            if mark {
                "Vencord detected"
            } else {
                "No Vencord marker"
            },
        );
        self.set_page(self.page);
    }
    fn set_page(&mut self, p: u8) {
        if self.recording.is_some() {
            return;
        }
        self.clear_worker();
        self.page = p;
        for v in self.widgets.values() {
            if v.page != 0 {
                unsafe {
                    ShowWindow(v.h, if v.page == p { SW_SHOW } else { SW_HIDE });
                }
            }
        }
        self.enable(LAUNCH, p != 1);
        self.enable(COMPARE, p != 2);
        unsafe {
            KillTimer(self.h, 1);
        }
        if p == 2 {
            self.enable(RECORD, false);
            self.text(32, "Checking the current session...");
            self.text(41, "");
            self.slots();
        }
        self.arm(true);
    }
    fn arm(&mut self, immediate: bool) {
        let minimized = unsafe { IsIconic(self.h) != 0 };
        unsafe {
            KillTimer(self.h, 1);
        }
        if minimized && self.recording.is_none() {
            return;
        }
        let interval = if self.recording.is_some() {
            5000
        } else if self.page == 1 {
            6000
        } else {
            8000
        };
        unsafe {
            SetTimer(self.h, 1, interval, None);
        }
        if immediate && self.recording.is_none() {
            self.tick();
        }
    }
    fn tick(&mut self) {
        if self.page == 1 {
            let present = self.c().present();
            let (state, desc, ready) = match present {
                Err(e) => ("Cannot check Discord", e, false),
                Ok(true) => (
                    "Discord is running",
                    "To switch profiles, right-click Discord in the Windows tray and choose Quit Discord.".into(),
                    false,
                ),
                Ok(false) if self.installed.is_some() => (
                    "Ready to launch",
                    "Relay exits after launch. Your usual Discord shortcut still works.".into(),
                    true,
                ),
                Ok(false) => (
                    "Discord not found",
                    "Install Discord for Windows, then reopen Relay.".into(),
                    false,
                ),
            };
            self.text(25, state);
            self.text(26, desc);
            self.enable(START, ready);
            return;
        }
        if self.pending {
            return;
        }
        if self.recording.is_some() && Instant::now() < self.due {
            return;
        }
        if self.worker.is_none() {
            self.worker = Some(Worker::new(self.h));
        }
        self.pending = self
            .worker
            .as_ref()
            .unwrap()
            .tx
            .send(Job {
                seq: self.seq,
                client: self.c().clone(),
            })
            .is_ok();
    }
    fn slots(&mut self) {
        let v = self
            .live
            .as_ref()
            .and_then(|s| s.version.as_ref())
            .or(self.installed.as_ref())
            .cloned()
            .unwrap_or_default();
        let mut values = Vec::new();
        for (value_id, stamp_id, mode) in [
            (36, 38, Mode::Standard),
            (37, 39, Mode::Saver),
            (43, 44, Mode::Strong),
        ] {
            let r = readings::load(&self.home, self.c().name, &v, mode);
            self.text(
                value_id,
                r.as_ref()
                    .map(|r| readings::memory(r.ram))
                    .unwrap_or_else(|| "Not recorded".into()),
            );
            self.text(
                stamp_id,
                r.as_ref()
                    .map(|r| {
                        let stamp = chrono::DateTime::parse_from_rfc3339(&r.utc)
                            .map(|t| {
                                t.with_timezone(&chrono::Utc)
                                    .format("%b %d, %H:%M UTC")
                                    .to_string()
                            })
                            .unwrap_or_default();
                        format!("{stamp}\r\n{} committed", readings::memory(r.commit))
                    })
                    .unwrap_or_default(),
            );
            values.push(r);
        }
        self.text(
            40,
            match (&values[0], &values[1], &values[2]) {
                (_, Some(old), Some(new)) => readings::difference(old.ram, new.ram, "Balanced"),
                (Some(old), None, Some(new)) => readings::difference(old.ram, new.ram, "Stock"),
                _ => "Record Strong saver and a baseline to compare.".into(),
            },
        );
    }
    fn finish_record(&mut self, msg: String) {
        self.recording = None;
        self.notice = Some(msg);
        self.text(RECORD, "Record 30 seconds");
        self.enable(CHANNEL, true);
        self.enable(LAUNCH, true);
        self.arm(false);
    }
    fn received(&mut self) {
        let Some((seq, res)) = self.worker.as_ref().and_then(|w| w.rx.try_recv().ok()) else {
            return;
        };
        if seq != self.seq {
            return;
        }
        self.pending = false;
        match res {
            Err(e) => {
                self.live = None;
                if self.recording.is_some() {
                    self.finish_record("Recording stopped. Previous reading kept.".into());
                }
                self.text(32, "Memory counters unavailable");
                self.text(41, e);
                self.enable(RECORD, false);
            }
            Ok(s) => {
                self.text(
                    32,
                    if !s.running {
                        "Discord is not running".into()
                    } else {
                        s.ram
                            .map(|r| {
                                format!(
                                    "{} total / {} renderer / {} processes",
                                    readings::memory(r),
                                    s.renderer_ram
                                        .map(readings::memory)
                                        .unwrap_or_else(|| "unknown".into()),
                                    s.count
                                )
                            })
                            .unwrap_or_else(|| "Waiting for memory counters...".into())
                    },
                );
                self.live = Some(s.clone());
                if let Some((initial, rows)) = &mut self.recording {
                    if !initial.matches(&s) {
                        self.finish_record("Discord changed. No reading was saved.".into())
                    } else {
                        rows.push(s.clone());
                        let n = rows.len();
                        if n == 7 {
                            let outcome = readings::summarize(rows).and_then(|r| {
                                readings::save(
                                    &self.home,
                                    self.c().name,
                                    s.version.as_deref().unwrap_or(""),
                                    s.mode,
                                    &r,
                                )
                            });
                            self.finish_record(match outcome {
                                Ok(()) => format!("{} reading saved.", s.mode.label()),
                                Err(e) => format!("Not saved: {e}"),
                            });
                        } else {
                            self.text(RECORD, format!("Sample {n} of 7"));
                            self.due = Instant::now() + Duration::from_secs(4);
                        }
                    }
                }
                self.slots();
                let note = if self.recording.is_some() {
                    "Recording. Close Relay to cancel.".into()
                } else if let Some(n) = &self.notice {
                    n.clone()
                } else if !s.running {
                    "Launch a profile, then reopen Relay to record it.".into()
                } else if s.mode == Mode::Unknown {
                    "Cannot verify the renderer profile yet. Wait for Discord to finish starting."
                        .into()
                } else {
                    format!(
                        "{} session. A new recording replaces its previous reading.",
                        s.mode.label()
                    )
                };
                self.text(41, note);
                self.enable(RECORD, self.recording.is_none() && s.matches(&s));
            }
        }
    }
    fn command(&mut self, id: i32, notify: u16) {
        match (id, notify) {
            (LAUNCH, 0) => self.set_page(1),
            (COMPARE, 0) => self.set_page(2),
            (CHANNEL, 1) => {
                let n = unsafe { SendMessageW(self.id(CHANNEL), CB_GETCURSEL, 0, 0) };
                if n >= 0 && self.recording.is_none() {
                    self.select(n as usize)
                }
            }
            (START, 0) => {
                if self.page != 1 {
                    return;
                }
                let mode = if unsafe { SendMessageW(self.id(SAVER), BM_GETCHECK, 0, 0) } == 1 {
                    Mode::Strong
                } else if unsafe { SendMessageW(self.id(BALANCED), BM_GETCHECK, 0, 0) } == 1 {
                    Mode::Saver
                } else {
                    Mode::Standard
                };
                match self.c().start(mode) {
                    Ok(()) => unsafe {
                        PostMessageW(self.h, WM_CLOSE, 0, 0);
                    },
                    Err(e) => {
                        self.tick();
                        self.text(26, e);
                    }
                }
            }
            (RECORD, 0) => {
                if self.page != 2 || self.recording.is_some() {
                    return;
                }
                let Some(s) = self.live.clone().filter(|s| s.matches(s)) else {
                    return;
                };
                self.clear_worker();
                self.live = Some(s.clone());
                self.recording = Some((s, Vec::new()));
                self.notice = None;
                self.enable(RECORD, false);
                self.enable(LAUNCH, false);
                self.enable(CHANNEL, false);
                self.text(41, "Recording. Close Relay to cancel.");
                self.due = Instant::now();
                self.arm(false);
                self.tick();
            }
            _ => {}
        }
    }
}
unsafe extern "system" fn proc(h: HWND, m: u32, wp: usize, lp: isize) -> isize {
    match m {
        WM_ERASEBKGND => {
            let dc = wp as HDC;
            let mut rect = zeroed();
            GetClientRect(h, &mut rect);
            let brush = GetStockObject(DC_BRUSH);
            SetDCBrushColor(dc, PAGE);
            FillRect(dc, &rect, brush);
            rect.right = (194 * GetDpiForSystem() as i32 + 48) / 96;
            SetDCBrushColor(dc, BLUE);
            FillRect(dc, &rect, brush);
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = wp as HDC;
            let id = GetDlgCtrlID(lp as HWND);
            let side = (10..=14).contains(&id);
            let bg = if side { BLUE } else { PAGE };
            SetBkColor(dc, bg);
            SetTextColor(
                dc,
                if side {
                    if id == 11 || id == 12 {
                        ICE
                    } else {
                        0xffffff
                    }
                } else if matches!(id, 22 | 23 | 24 | 26 | 33 | 38 | 39 | 41) {
                    FADE
                } else {
                    INK
                },
            );
            SetDCBrushColor(dc, bg);
            GetStockObject(DC_BRUSH) as isize
        }
        WM_COMMAND => {
            access(|a| a.command((wp & 65535) as i32, (wp >> 16) as u16));
            0
        }
        WM_TIMER | PING => {
            access(|a| a.tick());
            0
        }
        RESULT => {
            access(|a| a.received());
            0
        }
        WM_SIZE => {
            access(|a| a.arm(true));
            0
        }
        WM_DESTROY => {
            KillTimer(h, 1);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, m, wp, lp),
    }
}
pub fn run() -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(null());
        let cls = w("RelayNativeRust");
        let title = w("Relay | Discord launcher");
        // MAKEINTRESOURCEW(1): this is a resource ID, not an address to dereference.
        let ico = LoadIconW(instance, std::ptr::without_provenance::<u16>(1));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: cls.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hIcon: ico,
            ..zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(xerr());
        }
        let dpi = GetDpiForSystem() as i32;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (690 * dpi + 48) / 96,
            bottom: (466 * dpi + 48) / 96,
        };
        AdjustWindowRect(&mut rect, style, 0);
        let (ww, hh) = (rect.right - rect.left, rect.bottom - rect.top);
        let h = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            cls.as_ptr(),
            title.as_ptr(),
            style,
            (GetSystemMetrics(SM_CXSCREEN) - ww) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - hh) / 2,
            ww,
            hh,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if h.is_null() {
            return Err(xerr());
        }
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("Local application data directory unavailable")?;
        let mut app = App {
            h,
            dpi,
            widgets: HashMap::new(),
            fonts: vec![],
            clients: discord::clients(&local),
            idx: 0,
            home: local.join("RelayLauncher/readings"),
            page: 1,
            installed: None,
            live: None,
            worker: None,
            seq: 0,
            pending: false,
            recording: None,
            due: Instant::now(),
            notice: None,
        };
        app.init()?;
        APP.with(|a| *a.borrow_mut() = Some(app));
        ShowWindow(h, SW_SHOWNORMAL);
        UpdateWindow(h);
        let mut msg = zeroed();
        loop {
            let n = GetMessageW(&mut msg, null_mut(), 0, 0);
            if n == 0 {
                break;
            }
            if n < 0 {
                return Err(xerr());
            }
            if IsDialogMessageW(h, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        APP.with(|a| {
            a.borrow_mut().take();
        });
        Ok(())
    }
}
