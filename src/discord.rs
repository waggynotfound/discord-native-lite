use crate::readings::{version, Mode, Snapshot};
use serde::Deserialize;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::Read,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, LocalFree, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::*,
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION},
    },
    UI::Shell::CommandLineToArgvW,
};
use wmi::{COMLibrary, WMIConnection};

pub const HINT: &str = "--js-flags=--optimize-for-size";
// One Windows argv element. V8's growth factor is a GC tuning parameter, not a heap ceiling.
pub const STRONG_HINT: &str =
    "--js-flags=--optimize-for-size --memory-saver-mode --heap-growing-percent=15";
#[derive(Debug, Clone)]
pub struct Client {
    pub name: &'static str,
    pub image: &'static str,
    pub root: PathBuf,
}
pub fn clients(local: &Path) -> Vec<Client> {
    let all = [
        ("Stable", "Discord"),
        ("Canary", "DiscordCanary"),
        ("PTB", "DiscordPTB"),
    ]
    .map(|(name, image)| Client {
        name,
        image,
        root: local.join(image),
    });
    let mut a = all
        .iter()
        .filter(|c| c.root.is_dir())
        .cloned()
        .collect::<Vec<_>>();
    if a.is_empty() {
        a.push(all[0].clone())
    }
    a
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn session(pid: u32) -> Option<u32> {
    let mut s = 0;
    (unsafe { ProcessIdToSessionId(pid, &mut s) } != 0).then_some(s)
}
struct Handle(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn current_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid));
        if h.0.is_null() {
            return None;
        }
        let mut b = vec![0u16; 32768];
        let mut n = b.len() as u32;
        (QueryFullProcessImageNameW(h.0, 0, b.as_mut_ptr(), &mut n) != 0)
            .then(|| PathBuf::from(String::from_utf16_lossy(&b[..n as usize])))
    }
}
pub fn vencord(exe: &Path) -> bool {
    let Some(p) = exe.parent() else { return false };
    let Ok(f) = fs::File::open(p.join("resources/app.asar")) else {
        return false;
    };
    if !f.metadata().is_ok_and(|m| m.len() <= 65536) {
        return false;
    }
    let mut b = Vec::new();
    f.take(65537).read_to_end(&mut b).is_ok()
        && b.len() <= 65536
        && b.windows(7).any(|x| x.eq_ignore_ascii_case(b"vencord"))
}
impl Client {
    pub fn latest(&self) -> Result<Option<(String, PathBuf)>, String> {
        let dirs = match fs::read_dir(&self.root) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(err(e)),
        };
        let mut best = None;
        for d in dirs {
            let d = d.map_err(err)?;
            let name = d.file_name();
            let Some(raw) = name.to_str().and_then(|n| n.strip_prefix("app-")) else {
                continue;
            };
            let Some(v) = version(raw) else { continue };
            let exe = d.path().join(format!("{}.exe", self.image));
            if exe.is_file() && best.as_ref().is_none_or(|(last, _, _)| v > *last) {
                best = Some((v, raw.to_string(), exe))
            }
        }
        Ok(best.map(|(_, v, p)| (v, p)))
    }
    fn owns(&self, path: &Path) -> bool {
        fn eq(a: &OsStr, b: &OsStr) -> bool {
            a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
        }
        let Some(dir) = path.parent() else {
            return false;
        };
        if !path
            .file_name()
            .is_some_and(|s| eq(s, OsStr::new(&format!("{}.exe", self.image))))
        {
            return false;
        }
        eq(dir.as_os_str(), self.root.as_os_str())
            || (dir
                .parent()
                .is_some_and(|d| eq(d.as_os_str(), self.root.as_os_str()))
                && build(path).is_some())
    }
    // Launch view: read only process names; open candidate handles only. No WMI or RAM counters.
    pub fn present(&self) -> Result<bool, String> {
        unsafe {
            let mine = session(std::process::id()).ok_or("Windows session unavailable")?;
            let h = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
            if h.0 == INVALID_HANDLE_VALUE {
                return Err(err(std::io::Error::last_os_error()));
            }
            let mut p: PROCESSENTRY32W = std::mem::zeroed();
            p.dwSize = std::mem::size_of_val(&p) as u32;
            let mut ok = Process32FirstW(h.0, &mut p);
            let image = format!("{}.exe", self.image);
            while ok != 0 {
                let l = p
                    .szExeFile
                    .iter()
                    .position(|&x| x == 0)
                    .unwrap_or(p.szExeFile.len());
                if l == image.len()
                    && p.szExeFile[..l]
                        .iter()
                        .zip(image.bytes())
                        .all(|(&a, b)| a < 128 && (a as u8).eq_ignore_ascii_case(&b))
                    && session(p.th32ProcessID).is_none_or(|s| s == mine)
                    && current_path(p.th32ProcessID).is_none_or(|p| self.owns(&p))
                {
                    return Ok(true);
                }
                ok = Process32NextW(h.0, &mut p);
            }
            if GetLastError() != ERROR_NO_MORE_FILES {
                return Err(err(std::io::Error::last_os_error()));
            }
            Ok(false)
        }
    }
    fn command(&self, mode: Mode, running: bool) -> Result<Command, String> {
        if running {
            return Err("Quit Discord from its tray icon before switching profiles.".into());
        }
        let (_, exe) = self
            .latest()?
            .ok_or("Discord is not installed for this channel.")?;
        let mut cmd = Command::new(&exe);
        cmd.current_dir(exe.parent().unwrap());
        match mode {
            Mode::Strong => {
                cmd.arg(STRONG_HINT);
            }
            Mode::Saver => {
                cmd.arg(HINT);
            }
            Mode::Standard => (),
            Mode::Unknown => return Err("Choose a launch profile first.".into()),
        }
        Ok(cmd)
    }
    pub fn start(&self, mode: Mode) -> Result<(), String> {
        self.command(mode, self.present()?)?.spawn().map_err(err)?;
        Ok(())
    }
}
fn build(exe: &Path) -> Option<String> {
    let s = exe.parent()?.file_name()?.to_str()?.strip_prefix("app-")?;
    version(s)?;
    Some(s.into())
}
fn argv(s: &str) -> Vec<String> {
    let w = OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        let mut n = 0;
        let p = CommandLineToArgvW(w.as_ptr(), &mut n);
        if p.is_null() {
            return vec![];
        }
        let mut v = Vec::with_capacity(n as usize);
        for &arg in std::slice::from_raw_parts(p, n as usize) {
            let mut len = 0;
            while *arg.add(len) != 0 {
                len += 1
            }
            v.push(String::from_utf16_lossy(std::slice::from_raw_parts(
                arg, len,
            )))
        }
        LocalFree(p.cast());
        v
    }
}
fn classify(complete: bool, main: Mode, renderers: &[Mode]) -> Mode {
    if complete && !renderers.is_empty() && renderers.iter().all(|&x| x == main) {
        main
    } else {
        Mode::Unknown
    }
}
fn profile(args: &[String]) -> Mode {
    let mut mask = 0;
    for flag in args
        .iter()
        .filter_map(|s| s.strip_prefix("--js-flags="))
        .flat_map(str::split_ascii_whitespace)
    {
        let flag = flag.replace('_', "-");
        match flag.as_str() {
            "--optimize-for-size" => mask |= 1,
            "--memory-saver-mode" => mask |= 2,
            "--heap-growing-percent=15" => mask |= 4,
            s if matches!(
                s.split('=').next().unwrap_or(""),
                "--memory-saver-mode"
                    | "--no-memory-saver-mode"
                    | "--nomemory-saver-mode"
                    | "--heap-growing-percent"
                    | "--optimize-for-size"
                    | "--no-optimize-for-size"
                    | "--nooptimize-for-size"
            ) =>
            {
                return Mode::Unknown
            }
            _ => (),
        }
    }
    match mask {
        0 => Mode::Standard,
        1 => Mode::Saver,
        7 => Mode::Strong,
        _ => Mode::Unknown,
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Proc {
    process_id: u32,
    executable_path: Option<String>,
    command_line: Option<String>,
    creation_date: Option<String>,
}
#[derive(Deserialize)]
struct Counter {
    #[serde(rename = "IDProcess")]
    id: u32,
    #[serde(rename = "WorkingSetPrivate")]
    ram: Option<u64>,
    #[serde(rename = "PrivateBytes")]
    commit: Option<u64>,
}
pub struct Meter(WMIConnection, u32);
impl Meter {
    pub fn new() -> Result<Self, String> {
        Ok(Self(
            WMIConnection::new(COMLibrary::new().map_err(err)?).map_err(err)?,
            session(std::process::id()).ok_or("Cannot identify Windows session")?,
        ))
    }
    pub fn sample(&self, c: &Client) -> Result<Snapshot, String> {
        let processes_query = format!(
            "SELECT ProcessId,ExecutablePath,CommandLine,CreationDate \
             FROM Win32_Process WHERE Name='{}.exe' AND SessionId={}",
            c.image, self.1,
        );
        let rows: Vec<Proc> = self.0.raw_query(processes_query).map_err(err)?;
        let mut s = Snapshot::default();
        let mut ids = HashSet::new();
        let mut renderers = Vec::new();
        let mut renderer_ids = HashSet::new();
        let (mut full, mut main_hint, mut mains) = (true, Mode::Unknown, 0);
        for p in rows {
            let Some(path) = p
                .executable_path
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
            else {
                s.running = true;
                full = false;
                continue;
            };
            if !c.owns(&path) {
                continue;
            }
            s.running = true;
            ids.insert(p.process_id);
            if build(&path).is_none() {
                full = false;
                continue;
            }
            let Some(cmd) = p.command_line.filter(|x| !x.trim().is_empty()) else {
                full = false;
                continue;
            };
            let a = argv(&cmd);
            if a.is_empty() {
                full = false;
                continue;
            }
            match a.iter().find_map(|x| x.strip_prefix("--type=")) {
                Some("renderer") => {
                    renderer_ids.insert(p.process_id);
                    renderers.push(profile(&a));
                }
                Some(_) => {}
                None => {
                    mains += 1;
                    main_hint = profile(&a);
                    s.version = build(&path);
                    s.session = p
                        .creation_date
                        .filter(|x| !x.is_empty())
                        .map(|d| format!("{}@{d}", p.process_id));
                    if s.session.is_none() {
                        full = false
                    }
                }
            }
        }
        s.count = ids.len();
        s.mode = classify(full && mains == 1, main_hint, &renderers);
        if !full || ids.is_empty() {
            return Ok(s);
        }
        let counters_query = format!(
            "SELECT IDProcess,WorkingSetPrivate,PrivateBytes \
             FROM Win32_PerfRawData_PerfProc_Process WHERE Name LIKE '{}%'",
            c.image,
        );
        let rows: Vec<Counter> = self.0.raw_query(counters_query).map_err(err)?;
        let (mut ram, mut commit, mut renderer_ram) = (0u64, 0u64, 0u64);
        for row in rows {
            if let (Some(r), Some(k)) = (row.ram, row.commit) {
                if ids.remove(&row.id) {
                    ram = ram.saturating_add(r);
                    commit = commit.saturating_add(k);
                    if renderer_ids.contains(&row.id) {
                        renderer_ram = renderer_ram.saturating_add(r);
                    }
                }
            }
        }
        if ids.is_empty() && ram > 0 && commit > 0 {
            s.ram = Some(ram);
            s.renderer_ram = (!renderer_ids.is_empty()).then_some(renderer_ram);
            s.commit = Some(commit)
        }
        Ok(s)
    }
}
#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn hint_and_mixed() {
        use Mode::*;
        assert_eq!(
            profile(&argv("client --js-flags=\"--foo --optimize-for-size\"")),
            Saver
        );
        assert_eq!(
            profile(&argv("client --js-flags=--optimize-for-size-extra")),
            Standard
        );
        assert_eq!(profile(&argv(&format!("client \"{STRONG_HINT}\""))), Strong);
        assert_eq!(
            profile(&argv(
                "client --js-flags=\"--memory-saver-mode --optimize-for-size\""
            )),
            Unknown
        );
        assert_eq!(
            profile(&argv(&format!(
                "client \"{STRONG_HINT} --no-memory-saver-mode\""
            ))),
            Unknown
        );
        assert_eq!(
            profile(&argv(&format!(
                "client \"{STRONG_HINT} --heap-growing-percent=30\""
            ))),
            Unknown
        );
        assert_eq!(classify(true, Strong, &[Saver]), Unknown);
        assert_eq!(classify(true, Standard, &[Standard]), Standard);
        assert_eq!(classify(true, Saver, &[Saver]), Saver);
        assert_eq!(classify(true, Strong, &[Strong, Strong]), Strong);
        assert_eq!(classify(true, Strong, &[Strong, Standard]), Unknown);
        assert_eq!(classify(false, Strong, &[]), Unknown);
    }
    #[test]
    fn paths_and_handoff() {
        let local = std::env::temp_dir().join(format!("Relay-fixture-{}", std::process::id()));
        let c = clients(&local).remove(0);
        assert_eq!(c.latest().unwrap(), None);
        for v in ["1.0.9", "1.0.10", "1.0.99", "broken"] {
            let dir = c.root.join(format!("app-{v}"));
            fs::create_dir_all(&dir).unwrap();
            if v != "1.0.99" {
                fs::write(dir.join("Discord.exe"), b"fixture").unwrap()
            }
        }
        let (v, p) = c.latest().unwrap().unwrap();
        assert_eq!(v, "1.0.10");
        assert!(c.owns(&p));
        assert!(!c.owns(&local.join("DiscordCanary/app-1.0.10/Discord.exe")));
        assert!(c.command(Mode::Strong, true).is_err());
        assert_eq!(
            c.command(Mode::Standard, false).unwrap().get_args().count(),
            0
        );
        assert_eq!(
            c.command(Mode::Strong, false)
                .unwrap()
                .get_args()
                .collect::<Vec<_>>(),
            [OsStr::new(STRONG_HINT)]
        );
        let cmd = c.command(Mode::Saver, false).unwrap();
        assert_eq!(cmd.get_program(), p.as_os_str());
        assert_eq!(cmd.get_args().collect::<Vec<_>>(), [OsStr::new(HINT)]);
        fs::create_dir(p.parent().unwrap().join("resources")).unwrap();
        let marker = p.parent().unwrap().join("resources/app.asar");
        fs::write(&marker, b"require('Vencord')").unwrap();
        assert!(vencord(&p));
        fs::write(marker, vec![7; 70000]).unwrap();
        assert!(!vencord(&p));
        fs::remove_dir_all(local).unwrap();
    }
}
