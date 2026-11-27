//! Pure-Rust X11/XWayland client. Draw on state changes, not at video frame rate.
use crate::{
    client::{self, Client, Mode, Snapshot},
    Options,
};
use fontdue::{Font, FontSettings};
use std::{
    collections::HashMap,
    error::Error,
    os::fd::AsRawFd,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{xproto::*, Event},
    wrapper::ConnectionExt as _,
    COPY_DEPTH_FROM_PARENT,
};
type E = Box<dyn Error>;
const W: u16 = 738;
const H: u16 = 464;
const NAV: i32 = 182;
const BG: u32 = 0xf8fafd;
const BLUE: u32 = 0x2b3e8a;
const LIGHT: u32 = 0xe6ecf7;
const WHITE: u32 = 0xffffff;
const INK: u32 = 0x182540;
const SUB: u32 = 0x495d77;
const ICE: u32 = 0xc7e5ff;
struct Canvas {
    data: Vec<u8>,
    font: Font,
    cache: HashMap<(char, u8), (fontdue::Metrics, Vec<u8>)>,
}
impl Canvas {
    fn new() -> Result<Self, E> {
        let font = Font::from_bytes(
            include_bytes!("../assets/AtkinsonHyperlegible-Regular.ttf") as &[u8],
            FontSettings::default(),
        )?;
        Ok(Self {
            data: vec![0; W as usize * H as usize * 4],
            font,
            cache: HashMap::new(),
        })
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let b = c.to_le_bytes();
        for j in y.max(0)..(y + h).min(H as i32) {
            let a = x.max(0);
            let end = (x + w).min(W as i32);
            if a >= end {
                continue;
            }
            let start = (j * W as i32 + a) as usize * 4;
            for px in self.data[start..start + (end - a) as usize * 4].chunks_exact_mut(4) {
                px.copy_from_slice(&b)
            }
        }
    }
    fn boxline(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.rect(x, y, w, 2, c);
        self.rect(x, y + h - 2, w, 2, c);
        self.rect(x, y, 2, h, c);
        self.rect(x + w - 2, y, 2, h, c)
    }
    fn text(&mut self, x: i32, base: i32, s: &str, px: u8, col: u32, max: i32) {
        let rgb = col.to_le_bytes();
        let mut pen = x as f32;
        for ch in s.chars() {
            let (m, g) = self
                .cache
                .entry((ch, px))
                .or_insert_with(|| self.font.rasterize(ch, px as f32));
            let left = pen.round() as i32 + m.xmin;
            if left + m.width as i32 > x + max {
                break;
            }
            let top = base - m.height as i32 - m.ymin;
            for j in 0..m.height {
                for i in 0..m.width {
                    let a = g[j * m.width + i] as u32;
                    if a == 0 {
                        continue;
                    }
                    let (x, y) = (left + i as i32, top + j as i32);
                    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                        continue;
                    }
                    let at = ((y * W as i32 + x) * 4) as usize;
                    for (k, &fg) in rgb.iter().take(3).enumerate() {
                        let orig = self.data[at + k] as u32;
                        self.data[at + k] = ((u32::from(fg) * a + orig * (255 - a)) / 255) as u8
                    }
                }
            }
            pen += m.advance_width;
        }
    }
}
#[derive(Clone, Copy)]
struct Hit {
    r: (i32, i32, i32, i32),
    id: u8,
    on: bool,
}
struct View {
    opts: Options,
    c: Option<Client>,
    marker: bool,
    meter: bool,
    mode: Mode,
    snap: Snapshot,
    notice: String,
    focus: u8,
    hits: Vec<Hit>,
    samples: Vec<Snapshot>,
    build: Option<String>,
    due: Instant,
    saved: [Option<u64>; 3],
}
impl View {
    fn new(opts: Options) -> Self {
        let mut v = Self {
            opts,
            c: None,
            marker: false,
            meter: false,
            mode: Mode::Strong,
            snap: Snapshot {
                processes: 0,
                mode: Mode::Unknown,
                kib: None,
                session: None,
                uncertain: false,
            },
            notice: String::new(),
            focus: 0,
            hits: vec![],
            samples: vec![],
            build: None,
            due: Instant::now(),
            saved: [None; 3],
        };
        v.select();
        v
    }
    fn select(&mut self) {
        self.c = self.opts.resolve();
        self.marker = self.c.as_ref().is_some_and(Client::vencord);
        self.notice.clear();
        if self.c.is_none() {
            self.snap = Snapshot {
                processes: 0,
                mode: Mode::Unknown,
                kib: None,
                session: None,
                uncertain: false,
            };
        }
        self.read_slots();
        self.refresh();
    }
    fn read_slots(&mut self) {
        self.saved = [Mode::Stock, Mode::Balanced, Mode::Strong]
            .map(|m| self.c.as_ref().and_then(|c| client::load(c, m)))
    }
    fn refresh(&mut self) {
        let Some(c) = &self.c else {
            self.due = Instant::now() + Duration::from_secs(5);
            return;
        };
        self.due = Instant::now() + Duration::from_secs(if self.build.is_some() { 5 } else { 4 });
        match c.inspect(self.meter) {
            Ok(s) => {
                self.snap = s.clone();
                if let Some(b) = &self.build {
                    if s.mode == Mode::Unknown
                        || s.kib.is_none()
                        || self
                            .samples
                            .first()
                            .is_some_and(|p| p.session != s.session || p.mode != s.mode)
                    {
                        self.build = None;
                        self.samples.clear();
                        self.notice = "Session or counters changed. No reading saved.".into();
                    } else {
                        self.samples.push(s);
                        if self.samples.len() == 7 {
                            self.notice =
                                match client::save(c, b, self.samples[0].mode, &self.samples) {
                                    Ok(v) => {
                                        format!("Saved {} median private RAM.", client::mib(v))
                                    }
                                    Err(e) => e,
                                };
                            self.samples.clear();
                            self.build = None;
                            self.read_slots()
                        }
                    }
                }
            }
            Err(e) => {
                self.snap.uncertain = true;
                self.snap.kib = None;
                self.snap.mode = Mode::Unknown;
                self.notice = e;
                self.build = None;
                self.samples.clear();
            }
        }
    }
    fn button(
        &mut self,
        p: &mut Canvas,
        r: (i32, i32, i32, i32),
        id: u8,
        t: &str,
        on: bool,
        bg: u32,
        fg: u32,
    ) {
        let (x, y, w, h) = r;
        p.rect(x, y, w, h, bg);
        p.text(x + 12, y + h / 2 + 6, t, 16, fg, w - 20);
        if self.focus == id && on {
            p.boxline(
                x + 1,
                y + 1,
                w - 2,
                h - 2,
                if bg == BLUE { ICE } else { 0x6481bc },
            )
        }
        self.hits.push(Hit { r, id, on })
    }
    fn draw(&mut self, p: &mut Canvas) {
        self.hits.clear();
        p.rect(0, 0, W as i32, H as i32, BG);
        p.rect(0, 0, NAV, H as i32, BLUE);
        p.text(20, 60, "Relay", 35, WHITE, 150);
        p.text(21, 89, "Discord launcher", 16, ICE, 155);
        let active = self.build.is_some();
        for (i, t) in ["Launch", "Compare RAM"].iter().enumerate() {
            let on = self.meter == (i == 1);
            self.button(
                p,
                (12, 133 + 46 * i as i32, 157, 37),
                i as u8,
                t,
                !active,
                if on { 0x455ca0 } else { BLUE },
                WHITE,
            )
        }
        p.text(20, 322, "Discord channel", 14, ICE, 146);
        self.button(
            p,
            (15, 334, 151, 37),
            2,
            &format!("{}   ›", client::CHANNELS[self.opts.channel]),
            !active && self.opts.path.is_none(),
            LIGHT,
            INK,
        );
        p.text(
            21,
            402,
            if self.c.is_some() {
                "Native install found"
            } else {
                "Not installed"
            },
            14,
            WHITE,
            150,
        );
        p.text(
            21,
            426,
            if self.marker {
                "Vencord detected"
            } else {
                "No Vencord marker"
            },
            14,
            ICE,
            152,
        );
        if !self.meter {
            p.text(208, 63, "Launch Discord", 32, INK, 502);
            p.text(
                210,
                96,
                "Same desktop client, account and plugins.",
                17,
                SUB,
                505,
            );
            for (i, (m, caption)) in [
                (Mode::Strong, "Favor a smaller heap; may use more CPU."),
                (Mode::Balanced, "Prefer smaller JavaScript code and heaps."),
                (Mode::Stock, "Use Discord's normal startup settings."),
            ]
            .iter()
            .enumerate()
            {
                let y = 118 + 56 * i as i32;
                let marked = self.mode == *m;
                let bg = if marked { LIGHT } else { BG };
                p.rect(208, y, 501, 48, bg);
                p.boxline(221, y + 12, 13, 13, if marked { BLUE } else { SUB });
                if marked {
                    p.rect(224, y + 15, 7, 7, BLUE)
                }
                p.text(246, y + 21, m.label(), 17, INK, 446);
                p.text(246, y + 40, caption, 14, SUB, 442);
                let id = 3 + i as u8;
                if self.focus == id {
                    p.boxline(208, y, 501, 48, 0x6481bc)
                }
                self.hits.push(Hit {
                    r: (208, y, 501, 48),
                    id,
                    on: true,
                })
            }
            let ready = self.c.is_some() && self.snap.processes == 0 && !self.snap.uncertain;
            let (status, tip) = if self.c.is_none() {
                (
                    "Discord not found",
                    "Install native Discord (.deb or tarball), or try another channel.",
                )
            } else if self.snap.uncertain {
                (
                    "Process status unavailable",
                    "Check permissions before launching another client.",
                )
            } else if !ready {
                (
                    "Discord is running",
                    "Quit Discord completely before switching profiles.",
                )
            } else {
                ("Ready to launch", "Relay exits after starting Discord.")
            };
            p.text(210, 324, status, 20, INK, 508);
            p.text(
                210,
                354,
                if self.notice.is_empty() {
                    tip
                } else {
                    &self.notice
                },
                15,
                SUB,
                507,
            );
            self.button(
                p,
                (209, 389, 194, 42),
                6,
                "Start Discord",
                ready,
                if ready { BLUE } else { LIGHT },
                if ready { WHITE } else { SUB },
            );
        } else {
            p.text(208, 63, "Compare RAM", 32, INK, 506);
            let live = if self.c.is_none() {
                "No native Discord installation".into()
            } else if self.snap.processes == 0 && !self.snap.uncertain {
                "Discord is not running".into()
            } else {
                self.snap
                    .kib
                    .map(|n| {
                        format!(
                            "{} private RAM  /  {} processes",
                            client::mib(n),
                            self.snap.processes
                        )
                    })
                    .unwrap_or_else(|| "Private RAM unavailable".into())
            };
            p.text(210, 103, &live, 20, INK, 507);
            p.text(
                210,
                143,
                "Use the same channel or call, and let each fresh launch",
                16,
                SUB,
                507,
            );
            p.text(
                210,
                166,
                "warm up for a minute before recording.",
                16,
                SUB,
                507,
            );
            p.rect(208, 199, 501, 94, LIGHT);
            for (i, m) in [Mode::Stock, Mode::Balanced, Mode::Strong]
                .iter()
                .enumerate()
            {
                let x = 222 + i as i32 * 165;
                p.text(x, 228, m.label(), 15, SUB, 155);
                p.text(
                    x,
                    269,
                    &self.saved[i]
                        .map(client::mib)
                        .unwrap_or_else(|| "No reading".into()),
                    22,
                    INK,
                    156,
                )
            }
            let live_ok = self.c.is_some()
                && self.snap.mode != Mode::Unknown
                && self.snap.kib.is_some()
                && !self.snap.uncertain;
            p.text(
                210,
                327,
                if self.snap.processes == 0 {
                    "No active profile"
                } else {
                    self.snap.mode.label()
                },
                19,
                INK,
                504,
            );
            let info = if active {
                format!("Sample {} of 7. Close Relay to cancel.", self.samples.len())
            } else if !self.notice.is_empty() {
                self.notice.clone()
            } else if live_ok {
                "Records the median over 30 seconds.".into()
            } else {
                "Needs a verified renderer and readable /proc memory counters.".into()
            };
            p.text(210, 356, &info, 15, SUB, 508);
            let on = live_ok && !active;
            self.button(
                p,
                (209, 389, 201, 42),
                7,
                if active {
                    "Recording…"
                } else {
                    "Record 30 seconds"
                },
                on,
                if on { BLUE } else { LIGHT },
                if on { WHITE } else { SUB },
            );
        }
    }
    fn act(&mut self, id: u8) -> bool {
        if !self.hits.iter().any(|b| b.id == id && b.on) {
            return false;
        }
        self.focus = id;
        match id {
            0 | 1 => {
                self.meter = id == 1;
                self.notice.clear();
                self.read_slots();
                self.refresh()
            }
            2 => {
                self.opts.channel = (self.opts.channel + 1) % 3;
                self.select()
            }
            3..=5 => self.mode = client::MODES[(id - 3) as usize],
            6 => {
                if let Some(c) = &self.c {
                    match c.launch(self.mode) {
                        Ok(()) => return true,
                        Err(e) => {
                            self.refresh();
                            self.notice = e
                        }
                    }
                }
            }
            7 => {
                if let Some(c) = &self.c {
                    match c.fingerprint() {
                        Ok(v) => {
                            self.build = Some(v);
                            self.samples.clear();
                            self.refresh()
                        }
                        Err(e) => self.notice = e,
                    }
                }
            }
            _ => {}
        }
        false
    }
    fn next(&mut self, back: bool) {
        let ids: Vec<_> = self.hits.iter().filter(|b| b.on).map(|b| b.id).collect();
        if ids.is_empty() {
            return;
        }
        let i = ids.iter().position(|&k| k == self.focus).unwrap_or(0);
        self.focus = ids[(i + if back { ids.len() - 1 } else { 1 }) % ids.len()]
    }
}
pub fn run(opts: Options) -> Result<(), E> {
    let (conn, scr) = x11rb::connect(None)
        .map_err(|_| "Cannot open an X11 display. Use an X11 or XWayland session.")?;
    let s = &conn.setup().roots[scr];
    let depth = s.root_depth;
    let v = s
        .allowed_depths
        .iter()
        .flat_map(|d| &d.visuals)
        .find(|v| v.visual_id == s.root_visual)
        .ok_or("No root visual")?;
    if conn.setup().image_byte_order != ImageOrder::LSB_FIRST
        || v.red_mask != 0xff0000
        || v.green_mask != 0xff00
        || v.blue_mask != 0xff
        || !conn
            .setup()
            .pixmap_formats
            .iter()
            .any(|p| p.depth == depth && p.bits_per_pixel == 32)
    {
        return Err("This display needs a 24/32-bit TrueColor visual.".into());
    }
    let win = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        s.root,
        0,
        0,
        W,
        H,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().background_pixel(BG).event_mask(
            EventMask::EXPOSURE
                | EventMask::BUTTON_PRESS
                | EventMask::KEY_PRESS
                | EventMask::STRUCTURE_NOTIFY,
        ),
    )?
    .check()?;
    conn.create_gc(gc, win, &CreateGCAux::new())?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"Relay | Discord launcher",
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"relay\0Relay\0",
    )?;
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let close = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    conn.change_property32(PropMode::REPLACE, win, protocols, AtomEnum::ATOM, &[close])?;
    let mut hints = [0u32; 18];
    hints[0] = (1 << 4) | (1 << 5);
    hints[5] = W.into();
    hints[6] = H.into();
    hints[7] = W.into();
    hints[8] = H.into();
    conn.change_property32(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NORMAL_HINTS,
        AtomEnum::WM_SIZE_HINTS,
        &hints,
    )?;
    conn.map_window(win)?;
    conn.flush()?;
    let mut app = View::new(opts);
    let mut canvas = Canvas::new()?;
    let mut dirty = true;
    let mut visible = true;
    loop {
        while let Some(e) = conn.poll_for_event()? {
            match e {
                Event::Expose(_) => dirty = true,
                Event::UnmapNotify(_) => visible = false,
                Event::MapNotify(_) => {
                    visible = true;
                    dirty = true
                }
                Event::ClientMessage(e)
                    if e.type_ == protocols && e.data.as_data32()[0] == close =>
                {
                    return Ok(())
                }
                Event::DestroyNotify(_) => return Ok(()),
                Event::ButtonPress(e) if e.detail == 1 => {
                    let (x, y) = (e.event_x as i32, e.event_y as i32);
                    if let Some(id) = app
                        .hits
                        .iter()
                        .find(|h| {
                            let (a, b, c, d) = h.r;
                            h.on && x >= a && x < a + c && y >= b && y < b + d
                        })
                        .map(|h| h.id)
                    {
                        if app.act(id) {
                            return Ok(());
                        }
                        dirty = true
                    }
                }
                Event::KeyPress(e) => {
                    let key = conn.get_keyboard_mapping(e.detail, 1)?.reply()?.keysyms[0];
                    match key {
                        0xff1b => return Ok(()),
                        0xff09 | 0xfe20 => app.next(e.state.contains(KeyButMask::SHIFT)),
                        0xff0d | 0x20 => {
                            if app.act(app.focus) {
                                return Ok(());
                            }
                        }
                        0xff54 | 0xff53 => app.next(false),
                        0xff52 | 0xff51 => app.next(true),
                        _ => {}
                    }
                    dirty = true
                }
                _ => {}
            }
        }
        if (visible || app.build.is_some()) && Instant::now() >= app.due {
            app.refresh();
            dirty = true
        }
        if dirty && visible {
            app.draw(&mut canvas);
            for y in (0..H).step_by(32) {
                let h = (H - y).min(32);
                let at = y as usize * W as usize * 4;
                conn.put_image(
                    ImageFormat::Z_PIXMAP,
                    win,
                    gc,
                    W,
                    h,
                    0,
                    y as i16,
                    0,
                    depth,
                    &canvas.data[at..at + h as usize * W as usize * 4],
                )?;
            }
            conn.flush()?;
            dirty = false
        }
        let timeout = if visible || app.build.is_some() {
            app.due
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(5000) as i32
                + 1
        } else {
            -1
        };
        let mut fd = libc::pollfd {
            fd: conn.stream().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let r = unsafe { libc::poll(&mut fd, 1, timeout) };
        if r < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(std::io::Error::last_os_error().into());
        }
    }
}
