mod client;
mod ui;
use client::{Client, Mode};
use std::{
    env,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
const HELP:&str="Relay 2.1.0 for Linux x86_64\n\nrelay [--channel stable|canary|ptb] [--discord /absolute/path/to/Discord]\n      [--status | --launch strong|balanced|stock | --record]\n\nNo action opens the graphical launcher (X11 or XWayland). --record measures\nseven samples over 30 seconds without launching or closing Discord.\nNative .deb/tarball installations supported. Flatpak and Snap are not.\nReadings: $XDG_STATE_HOME/relay/readings or ~/.local/state/relay/readings.";
#[derive(Clone)]
struct Options {
    channel: usize,
    path: Option<PathBuf>,
}
impl Options {
    fn resolve(&self) -> Option<Client> {
        if let Some(p) = &self.path {
            client::validate(p, self.channel)
        } else {
            client::discover(self.channel)
        }
    }
}
fn run() -> client::Result<()> {
    let mut a = env::args_os().skip(1);
    let mut opts = Options {
        channel: 0,
        path: None,
    };
    let mut action = None;
    let mut mode = Mode::Strong;
    while let Some(arg) = a.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!("{HELP}");
                return Ok(());
            }
            Some("--version") => {
                println!("Relay {} (Linux x86_64)", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--discord") => {
                let p = PathBuf::from(a.next().ok_or("--discord needs a path")?);
                if !p.is_absolute() {
                    return Err(
                        "--discord requires an absolute path to the native executable".into(),
                    );
                }
                opts.path = Some(p)
            }
            Some("--channel") => {
                let v = a.next().ok_or("--channel needs a name")?;
                opts.channel = client::CHANNELS
                    .iter()
                    .position(|c| v.to_string_lossy().eq_ignore_ascii_case(c))
                    .ok_or("Unknown channel")?
            }
            Some(s @ ("--status" | "--launch" | "--record")) => {
                if action.replace(s.to_owned()).is_some() {
                    return Err("Choose just one action".into());
                }
                if s == "--launch" {
                    let v = a.next().ok_or("--launch needs a profile")?;
                    mode = client::MODES
                        .into_iter()
                        .find(|m| m.slug() == v.to_string_lossy())
                        .ok_or("Unknown launch profile")?
                }
            }
            _ => {
                return Err(format!(
                    "Unknown argument: {}. See --help",
                    arg.to_string_lossy()
                ))
            }
        }
    }
    if opts.path.is_some() && opts.resolve().is_none() {
        return Err("Not a native executable for the selected channel. Specify the actual Discord ELF binary, not a wrapper.".into());
    }
    if action.is_none() {
        return ui::run(opts).map_err(|e| format!("{e}\nFor terminal use: relay --help"));
    }
    let c=opts.resolve().ok_or("No native Discord installation found. Install Discord or pass --discord /absolute/path/to/Discord.")?;
    match action.as_deref() {
        Some("--launch") => c.launch(mode),
        Some("--status") => {
            let s = c.inspect(true)?;
            println!("Channel: {}\nExecutable: {}\nVencord marker: {}\nProcesses: {}\nProfile: {}\nPrivate RAM: {}",client::CHANNELS[c.channel],c.path.display(),c.vencord(),s.processes,s.mode.label(),s.kib.map(client::mib).unwrap_or_else(||"unavailable".into()));
            Ok(())
        }
        Some("--record") => {
            let build = c.fingerprint()?;
            let mut rows = vec![];
            let at = Instant::now();
            for i in 0..7 {
                thread::sleep(Duration::from_secs(i * 5).saturating_sub(at.elapsed()));
                let s = c.inspect(true)?;
                if s.mode == Mode::Unknown
                    || s.kib.is_none()
                    || rows.first().is_some_and(|p: &client::Snapshot| {
                        p.mode != s.mode || p.session != s.session
                    })
                {
                    return Err(
                        "No stable, verified session with readable private RAM counters".into(),
                    );
                }
                rows.push(s)
            }
            let kb = client::save(&c, &build, rows[0].mode, &rows)?;
            println!(
                "{}: {} median private RAM",
                rows[0].mode.label(),
                client::mib(kb)
            );
            Ok(())
        }
        _ => unreachable!(),
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Relay: {e}");
        std::process::exit(1)
    }
}
