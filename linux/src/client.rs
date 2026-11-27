use std::{
    env, fs,
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
pub type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub const CHANNELS: [&str; 3] = ["Stable", "Canary", "PTB"];
pub const MODES: [Mode; 3] = [Mode::Strong, Mode::Balanced, Mode::Stock];
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Strong,
    Balanced,
    Stock,
    Unknown,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Strong => "Strong saver",
            Self::Balanced => "Balanced",
            Self::Stock => "Stock",
            Self::Unknown => "Unverified",
        }
    }
    pub fn arg(self) -> Option<&'static str> {
        match self {
            Self::Strong => {
                Some("--js-flags=--optimize-for-size --memory-saver-mode --heap-growing-percent=15")
            }
            Self::Balanced => Some("--js-flags=--optimize-for-size"),
            _ => None,
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Balanced => "balanced",
            Self::Stock => "stock",
            Self::Unknown => "unknown",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Client {
    pub path: PathBuf,
    pub channel: usize,
}
fn choices(c: usize) -> [&'static str; 2] {
    [
        ["Discord", "discord"],
        ["DiscordCanary", "discord-canary"],
        ["DiscordPTB", "discord-ptb"],
    ][c]
}
pub fn validate(path: &Path, c: usize) -> Option<Client> {
    let path = fs::canonicalize(path).ok()?;
    if !choices(c)
        .iter()
        .any(|name| path.file_name().is_some_and(|f| f == *name))
    {
        return None;
    }
    let m = path.metadata().ok()?;
    if !m.is_file() || m.permissions().mode() & 0o111 == 0 {
        return None;
    }
    let mut sig = [0u8; 4];
    fs::File::open(&path).ok()?.read_exact(&mut sig).ok()?;
    if sig != *b"\x7fELF" {
        return None;
    }
    Some(Client { path, channel: c })
}
pub fn discover(c: usize) -> Option<Client> {
    let [upper, lower] = choices(c);
    let mut roots = vec![
        PathBuf::from("/usr/share"),
        "/opt".into(),
        "/usr/lib".into(),
        "/usr/lib64".into(),
        "/usr/local/share".into(),
    ];
    if let Some(h) = env::var_os("HOME") {
        let h = PathBuf::from(h);
        roots.extend([
            h.join(".local/opt"),
            h.join(".local/share"),
            h.join("Applications"),
        ]);
    }
    for root in roots {
        for d in [upper, lower] {
            for n in [upper, lower] {
                if let Some(v) = validate(&root.join(d).join(n), c) {
                    return Some(v);
                }
            }
        }
    }
    for d in env::split_paths(&env::var_os("PATH").unwrap_or_default()).filter(|p| p.is_absolute())
    {
        for n in [upper, lower] {
            if let Some(v) = validate(&d.join(n), c) {
                return Some(v);
            }
        }
    }
    None
}
fn classify(args: &[u8]) -> Mode {
    let mut mask = 0;
    for a in args
        .split(|&b| b == 0)
        .filter_map(|a| a.strip_prefix(b"--js-flags="))
    {
        for flag in String::from_utf8_lossy(a).split_whitespace() {
            let f = flag.replace('_', "-");
            match f.as_str() {
                "--optimize-for-size" => mask |= 1,
                "--memory-saver-mode" => mask |= 2,
                "--heap-growing-percent=15" => mask |= 4,
                s if s.starts_with("--heap-growing-percent")
                    || s.contains("memory-saver-mode")
                    || s.contains("optimize-for-size") =>
                {
                    return Mode::Unknown
                }
                _ => {}
            }
        }
    }
    match mask {
        0 => Mode::Stock,
        1 => Mode::Balanced,
        7 => Mode::Strong,
        _ => Mode::Unknown,
    }
}
fn process_start(stat: &str) -> Option<&str> {
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)
}
fn private_kib(input: &str) -> Option<u64> {
    let mut clean = None;
    let mut dirty = None;
    let mut huge = 0;
    for l in input.lines() {
        let mut a = l.split_whitespace();
        let k = a.next()?;
        if !matches!(k, "Private_Clean:" | "Private_Dirty:" | "Private_Hugetlb:") {
            continue;
        }
        let n = a.next()?.parse::<u64>().ok()?;
        if a.next() != Some("kB") {
            return None;
        }
        match k {
            "Private_Clean:" => clean = Some(n),
            "Private_Dirty:" => dirty = Some(n),
            _ => huge = n,
        }
    }
    clean?.checked_add(dirty?)?.checked_add(huge)
}
fn same_exe(link: &Path, exe: &Path) -> bool {
    link == exe
        || link.as_os_str().as_bytes().strip_suffix(b" (deleted)")
            == Some(exe.as_os_str().as_bytes())
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub processes: usize,
    pub mode: Mode,
    pub kib: Option<u64>,
    pub session: Option<String>,
    pub uncertain: bool,
}
impl Client {
    pub fn vencord(&self) -> bool {
        let Some(parent) = self.path.parent() else {
            return false;
        };
        for p in ["resources/app.asar", "resources/app/index.js"] {
            if let Ok(f) = fs::File::open(parent.join(p)) {
                if !f.metadata().is_ok_and(|m| m.len() <= 65536) {
                    continue;
                }
                let mut data = Vec::new();
                if f.take(65537).read_to_end(&mut data).is_ok()
                    && data.windows(7).any(|b| b.eq_ignore_ascii_case(b"vencord"))
                {
                    return true;
                }
            }
        }
        false
    }
    pub fn fingerprint(&self) -> Result<String> {
        let m = self.path.metadata().map_err(err)?;
        Ok(format!(
            "{:x}-{:x}-{:x}-{:x}-{:x}",
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec()
        ))
    }
    pub fn inspect(&self, memory: bool) -> Result<Snapshot> {
        let uid = unsafe { libc::geteuid() };
        let mut out = Snapshot {
            processes: 0,
            mode: Mode::Unknown,
            kib: None,
            session: None,
            uncertain: false,
        };
        let mut main = Vec::new();
        let mut renderers = Vec::new();
        let mut total = Some(0u64);
        for d in fs::read_dir("/proc").map_err(err)?.flatten() {
            let Ok(pid) = d.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if !d.metadata().is_ok_and(|m| m.uid() == uid) {
                continue;
            }
            let p = d.path();
            let link = match fs::read_link(p.join("exe")) {
                Ok(l) => l,
                Err(_) => {
                    if let Ok(name) = fs::read_to_string(p.join("comm")) {
                        if choices(self.channel)
                            .iter()
                            .any(|s| s.eq_ignore_ascii_case(name.trim()))
                            && p.exists()
                        {
                            out.uncertain = true
                        }
                    }
                    continue;
                }
            };
            if !same_exe(&link, &self.path) {
                continue;
            }
            out.processes += 1;
            if !memory {
                continue;
            }
            let argv = fs::read(p.join("cmdline")).unwrap_or_default();
            let role = argv
                .split(|&b| b == 0)
                .find_map(|s| s.strip_prefix(b"--type="));
            if argv.is_empty() {
                out.uncertain = true
            } else {
                match role {
                    Some(b"renderer") => renderers.push(classify(&argv)),
                    None => {
                        main.push(classify(&argv));
                        out.session = fs::read_to_string(p.join("stat"))
                            .ok()
                            .and_then(|s| process_start(&s).map(|t| format!("{pid}:{t}")));
                    }
                    _ => {}
                }
            }
            let kb = fs::read_to_string(p.join("smaps_rollup"))
                .ok()
                .and_then(|s| private_kib(&s));
            total = total.and_then(|x| x.checked_add(kb?));
        }
        if memory && out.processes > 0 && !out.uncertain {
            out.kib = total;
            if main.len() == 1
                && !renderers.is_empty()
                && renderers.iter().all(|m| *m == main[0])
                && out.session.is_some()
            {
                out.mode = main[0]
            }
        }
        Ok(out)
    }
    pub fn launch(&self, mode: Mode) -> Result<()> {
        if mode == Mode::Unknown {
            return Err("Select a launch profile.".into());
        }
        let s = self.inspect(false)?;
        if s.processes > 0 || s.uncertain {
            return Err("Quit this Discord client completely before switching profiles.".into());
        }
        let mut cmd = Command::new(&self.path);
        cmd.current_dir(self.path.parent().unwrap());
        if let Some(arg) = mode.arg() {
            cmd.arg(arg);
        }
        cmd.spawn().map_err(err)?;
        Ok(())
    }
}
pub fn state_root() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .map(|p| p.join("relay/readings"))
}
fn slot(home: &Path, c: &Client, build: &str, m: Mode) -> PathBuf {
    home.join(format!(
        "{}-{}-{}.tsv",
        CHANNELS[c.channel],
        build,
        m.slug()
    ))
}
pub fn load(c: &Client, m: Mode) -> Option<u64> {
    let p = slot(&state_root()?, c, &c.fingerprint().ok()?, m);
    let mut v = String::new();
    fs::File::open(p)
        .ok()?
        .take(128)
        .read_to_string(&mut v)
        .ok()?;
    let mut fields = v.trim().split('\t');
    fields.next()?.parse::<u64>().ok()?;
    let kb = fields.next()?.parse::<u64>().ok()?;
    (fields.next().is_none() && kb > 0).then_some(kb)
}
pub fn save(c: &Client, b: &str, m: Mode, rows: &[Snapshot]) -> Result<u64> {
    if rows.len() != 7
        || m == Mode::Unknown
        || rows[0].session.is_none()
        || rows.iter().any(|s| {
            s.uncertain
                || s.mode != m
                || s.session != rows[0].session
                || s.kib.is_none_or(|k| k == 0)
        })
        || c.fingerprint()? != b
    {
        return Err("Session, build, or counters changed. No reading was saved.".into());
    }
    let mut a: Vec<_> = rows.iter().filter_map(|s| s.kib).collect();
    a.sort_unstable();
    let kb = a[3];
    let home = state_root().ok_or("No user state directory.")?;
    fs::create_dir_all(&home).map_err(err)?;
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?;
    let tmp = home.join(format!(".{}-{}.tmp", std::process::id(), t.as_nanos()));
    let mut f = fs::File::create_new(&tmp).map_err(err)?;
    let result = (|| -> io::Result<()> {
        writeln!(f, "{}\t{kb}", t.as_secs())?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, slot(&home, c, b, m))
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(tmp);
        return Err(err(e));
    }
    Ok(kb)
}
pub fn mib(kib: u64) -> String {
    format!("{:.0} MiB", kib as f64 / 1024.)
}
#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn flags() {
        for m in MODES {
            assert_eq!(classify(m.arg().unwrap_or("").as_bytes()), m)
        }
        assert_eq!(classify(b"--js-flags=--memory-saver-mode"), Mode::Unknown);
        assert_eq!(
            classify(b"--js-flags=--optimize-for-size --no-optimize-for-size"),
            Mode::Unknown
        );
    }
    #[test]
    fn ram_excludes_shared() {
        assert_eq!(private_kib("Shared_Clean: 5000 kB\nPrivate_Clean: 50 kB\nPrivate_Dirty: 30 kB\nPrivate_Hugetlb: 4 kB\n"),Some(84));
        assert_eq!(private_kib("Rss: 50 kB\n"), None);
    }
    #[test]
    fn exact_path() {
        assert!(same_exe(
            Path::new("/opt/Discord (deleted)"),
            Path::new("/opt/Discord")
        ));
        assert!(!same_exe(
            Path::new("/tmp/Discord"),
            Path::new("/opt/Discord")
        ));
    }
}
