use chrono::{DateTime, Timelike, Utc};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Unknown,
    Standard,
    Saver,
    Strong,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unverified",
            Self::Standard => "Stock",
            Self::Saver => "Balanced",
            Self::Strong => "Strong saver",
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub running: bool,
    pub count: usize,
    pub session: Option<String>,
    pub version: Option<String>,
    pub mode: Mode,
    pub ram: Option<u64>,
    pub renderer_ram: Option<u64>,
    pub commit: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct Reading {
    pub utc: String,
    pub ram: u64,
    pub commit: u64,
}
impl Snapshot {
    pub fn matches(&self, other: &Self) -> bool {
        self.running
            && other.running
            && self.mode != Mode::Unknown
            && self.mode == other.mode
            && self.session.is_some()
            && self.session == other.session
            && self.version.is_some()
            && self.version == other.version
            && other.ram.is_some_and(|n| n > 0)
            && other.commit.is_some_and(|n| n > 0)
    }
}
pub fn summarize(samples: &[Snapshot]) -> Result<Reading, String> {
    if samples.len() != 7 || !samples.iter().all(|s| samples[0].matches(s)) {
        return Err("Discord changed or a counter was unavailable. No reading was saved.".into());
    }
    let mut ram = samples.iter().filter_map(|s| s.ram).collect::<Vec<_>>();
    let mut commit = samples.iter().filter_map(|s| s.commit).collect::<Vec<_>>();
    ram.sort_unstable();
    commit.sort_unstable();
    let now: DateTime<Utc> = std::time::SystemTime::now().into();
    let utc = format!(
        "{}.{:07}Z",
        now.format("%Y-%m-%dT%H:%M:%S"),
        now.nanosecond() / 100
    );
    Ok(Reading {
        utc,
        ram: ram[3],
        commit: commit[3],
    })
}
pub fn version(s: &str) -> Option<Vec<u32>> {
    let p = s
        .split('.')
        .map(|v| {
            (!v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
                .then(|| v.parse::<u32>().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    (2..=4).contains(&p.len()).then_some(p)
}
fn slot(home: &Path, channel: &str, build: &str, mode: Mode) -> Option<PathBuf> {
    if !matches!(channel, "Stable" | "Canary" | "PTB")
        || version(build).is_none()
        || mode == Mode::Unknown
    {
        return None;
    }
    Some(home.join(format!("{channel}-{build}-{mode:?}.tsv")))
}
pub fn load(home: &Path, channel: &str, build: &str, mode: Mode) -> Option<Reading> {
    let f = fs::File::open(slot(home, channel, build, mode)?).ok()?;
    let mut buf = String::new();
    f.take(1025).read_to_string(&mut buf).ok()?;
    if buf.len() > 1024 {
        return None;
    }
    let mut f = buf.trim().trim_start_matches('\u{feff}').split('\t');
    let utc = f.next()?;
    DateTime::parse_from_rfc3339(utc).ok()?;
    let ram = f.next()?.parse().ok()?;
    let commit = f.next()?.parse().ok()?;
    if f.next().is_some() || ram == 0 || commit == 0 {
        return None;
    }
    Some(Reading {
        utc: utc.into(),
        ram,
        commit,
    })
}
pub fn save(
    home: &Path,
    channel: &str,
    build: &str,
    mode: Mode,
    r: &Reading,
) -> Result<(), String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let path = slot(home, channel, build, mode).ok_or("Invalid reading slot")?;
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let tmp = home.join(format!(
        ".{}-{}-{}.tmp",
        std::process::id(),
        DateTime::<Utc>::from(std::time::SystemTime::now()).timestamp_micros(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let mut f = fs::File::create_new(&tmp).map_err(|e| e.to_string())?;
    let write = (|| {
        write!(f, "{}\t{}\t{}", r.utc, r.ram, r.commit)?;
        f.sync_all()
    })();
    drop(f);
    if let Err(e) = write.and_then(|()| fs::rename(&tmp, path)) {
        let _ = fs::remove_file(tmp);
        return Err(e.to_string());
    }
    Ok(())
}
pub fn memory(bytes: u64) -> String {
    format!("{:.0} MiB", bytes as f64 / 1048576.)
}
pub fn difference(a: u64, b: u64, baseline: &str) -> String {
    let d = a.abs_diff(b);
    if d < 524288 {
        "Less than 1 MiB apart".into()
    } else {
        format!(
            "Strong saver: {} {} than {baseline}",
            memory(d),
            if a > b { "less" } else { "more" }
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn set() -> Vec<Snapshot> {
        [50, 600, 40, 45, 60, 55, 65]
            .map(|x| Snapshot {
                running: true,
                count: 2,
                session: Some("a".into()),
                version: Some("1.0.1".into()),
                mode: Mode::Standard,
                ram: Some(x),
                renderer_ram: None,
                commit: Some(2 * x),
            })
            .to_vec()
    }
    #[test]
    fn medians() {
        let s = summarize(&set()).unwrap();
        assert_eq!((s.ram, s.commit), (55, 110));
        assert_eq!(s.utc.len(), 28);
    }
    #[test]
    fn reject_changes() {
        let a = set();
        assert!(summarize(&a[..6]).is_err());
        for i in 0..5 {
            let mut s = set();
            match i {
                0 => s[4].version = None,
                1 => s[3].session = None,
                2 => s[6].commit = None,
                3 => s[5].mode = Mode::Saver,
                _ => s[0].running = false,
            };
            assert!(summarize(&s).is_err());
        }
    }
    #[test]
    fn legacy_and_replace() {
        let h = std::env::temp_dir().join(format!("Relay-readings-{}", std::process::id()));
        fs::create_dir_all(&h).unwrap();
        let p = slot(&h, "Stable", "1.0.1", Mode::Standard).unwrap();
        fs::write(&p, "2026-11-26T09:38:31.1234567Z\t111\t222").unwrap();
        assert_eq!(
            load(&h, "Stable", "1.0.1", Mode::Standard).unwrap().ram,
            111
        );
        save(
            &h,
            "Stable",
            "1.0.1",
            Mode::Standard,
            &summarize(&set()).unwrap(),
        )
        .unwrap();
        assert_eq!(load(&h, "Stable", "1.0.1", Mode::Standard).unwrap().ram, 55);
        assert!(load(&h, "Canary", "1.0.1", Mode::Standard).is_none());
        assert!(load(&h, "Stable", "1.0.2", Mode::Standard).is_none());
        fs::write(&p, "bad").unwrap();
        assert!(load(&h, "Stable", "1.0.1", Mode::Standard).is_none());
        fs::remove_file(p).unwrap();
        fs::remove_dir(h).unwrap();
    }
    #[test]
    fn slot_and_delta() {
        assert!(slot(Path::new("."), "Stable", "../file", Mode::Saver).is_none());
        assert_ne!(
            slot(Path::new("."), "Stable", "1.0.1", Mode::Saver),
            slot(Path::new("."), "Stable", "1.0.1", Mode::Strong)
        );
        assert!(difference(0, 2097152, "Stock").contains("more than Stock"));
        assert!(difference(2097152, 0, "Balanced").contains("less than Balanced"));
        assert!(difference(55, 55, "Stock").starts_with("Less than"));
    }
}
