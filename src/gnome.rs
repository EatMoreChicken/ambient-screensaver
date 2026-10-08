use crate::photos;
use serde::{Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{self, Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use zbus::blocking::{Connection, Proxy};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const ENTRY_NAME: &str = "ambient-photos-idle.desktop";
const MANAGED_MARKER: &str = "X-AmbientPhotos-Managed=true";
const IDLE_PATH: &str = "/org/gnome/Mutter/IdleMonitor/Core";
const LOCK_PATH: &str = "/org/gnome/ScreenSaver";
const POLL: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
struct Config {
    idle_seconds: u64,
    photo_directories: Vec<PathBuf>,
}

struct Paths {
    binary: PathBuf,
    config: PathBuf,
    autostart: PathBuf,
}

fn xdg_dir(name: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(path) = env::var_os(name).map(PathBuf::from) {
        if path.is_absolute() {
            return Ok(path);
        }
    }
    let home = env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(fallback))
}

fn paths() -> Result<Paths> {
    let config_home = xdg_dir("XDG_CONFIG_HOME", ".config")?;
    let data_home = xdg_dir("XDG_DATA_HOME", ".local/share")?;
    Ok(Paths {
        binary: data_home.join("ambient-screensaver/bin/ambient-screensaver"),
        config: config_home.join("ambient-screensaver/gnome.json"),
        autostart: config_home.join("autostart").join(ENTRY_NAME),
    })
}

fn desktop_argument(value: &str) -> Result<String> {
    if value.contains(['\n', '\r']) {
        return Err("a desktop entry path contains a line break".into());
    }
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' => quoted.push_str("\\\""),
            '`' => quoted.push_str("\\`"),
            '$' => quoted.push_str("\\\\$"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn desktop_entry(binary: &Path) -> Result<String> {
    let binary = binary
        .to_str()
        .ok_or("the installed path is not valid UTF-8")?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Ambient Photos idle launcher\nComment=Start Ambient Photos when GNOME is idle\nOnlyShowIn=GNOME;\nNoDisplay=true\nTerminal=false\nExec={} \"gnome\" \"run\"\n{}\n",
        desktop_argument(binary)?,
        MANAGED_MARKER
    ))
}

fn is_managed(path: &Path) -> Result<bool> {
    Ok(path.exists()
        && fs::read_to_string(path)?
            .lines()
            .any(|line| line == MANAGED_MARKER))
}

fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("invalid installation path")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn copy_binary(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or("invalid binary installation path")?;
    fs::create_dir_all(parent)?;
    if destination.exists() && fs::canonicalize(source)? == fs::canonicalize(destination)? {
        return Ok(());
    }
    let temporary = destination.with_extension(format!("tmp-{}", process::id()));
    fs::copy(source, &temporary)?;
    let permissions = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&temporary, permissions)?;
    fs::rename(temporary, destination)?;
    Ok(())
}

fn install(args: &[String]) -> Result<()> {
    let mut idle_seconds = 120_u64;
    let mut directories = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--idle-seconds" => {
                index += 1;
                idle_seconds = args
                    .get(index)
                    .ok_or("--idle-seconds needs a positive number")?
                    .parse()
                    .map_err(|_| "--idle-seconds needs a positive whole number")?;
                if idle_seconds == 0 {
                    return Err("--idle-seconds must be positive".into());
                }
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}").into())
            }
            value => directories.push(fs::canonicalize(value)?),
        }
        index += 1;
    }
    if directories.is_empty() {
        return Err("provide at least one photo directory".into());
    }
    if photos::discover(&directories).is_empty() {
        return Err("no supported photos found in the supplied directories".into());
    }
    let locations = paths()?;
    if locations.autostart.exists() && !is_managed(&locations.autostart)? {
        return Err(format!(
            "refusing to replace an unmanaged entry: {}",
            locations.autostart.display()
        )
        .into());
    }
    let config = Config {
        idle_seconds,
        photo_directories: directories,
    };
    let executable = env::current_exe()?;
    copy_binary(&executable, &locations.binary)?;
    write_private(&locations.config, &serde_json::to_vec_pretty(&config)?)?;
    write_private(
        &locations.autostart,
        desktop_entry(&locations.binary)?.as_bytes(),
    )?;
    println!("Installed {}", locations.binary.display());
    println!("GNOME login entry: {}", locations.autostart.display());
    println!("Idle timeout: {idle_seconds}s; active at the next GNOME login");
    Ok(())
}

fn status() -> Result<()> {
    let locations = paths()?;
    if !is_managed(&locations.autostart)? || !locations.binary.exists() {
        println!("GNOME integration is not installed");
        return Ok(());
    }
    let config: Config = serde_json::from_slice(&fs::read(&locations.config)?)?;
    println!("Installed binary: {}", locations.binary.display());
    println!("GNOME login entry: {}", locations.autostart.display());
    println!("Idle timeout: {}s", config.idle_seconds);
    for directory in config.photo_directories {
        println!("Photos: {}", directory.display());
    }
    Ok(())
}

fn uninstall() -> Result<()> {
    let locations = paths()?;
    if locations.autostart.exists() && !is_managed(&locations.autostart)? {
        return Err("refusing to remove an unmanaged GNOME login entry".into());
    }
    for path in [&locations.autostart, &locations.config, &locations.binary] {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    println!("GNOME integration removed; a running copy ends when you log out");
    Ok(())
}

fn proxies(connection: &Connection) -> Result<(Proxy<'_>, Proxy<'_>)> {
    Ok((
        Proxy::new(
            connection,
            "org.gnome.Mutter.IdleMonitor",
            IDLE_PATH,
            "org.gnome.Mutter.IdleMonitor",
        )?,
        Proxy::new(
            connection,
            "org.gnome.ScreenSaver",
            LOCK_PATH,
            "org.gnome.ScreenSaver",
        )?,
    ))
}

fn check() -> Result<()> {
    let connection = Connection::session()?;
    let (idle, lock) = proxies(&connection)?;
    let idle_ms: u64 = idle.call("GetIdletime", &())?;
    let locked: bool = lock.call("GetActive", &())?;
    println!(
        "GNOME idle: {:.1}s; screen locked: {locked}",
        idle_ms as f64 / 1000.0
    );
    Ok(())
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn should_launch(armed: &mut bool, idle_ms: u64, idle_seconds: u64, locked: bool) -> bool {
    let threshold = idle_seconds.saturating_mul(1000);
    if idle_ms < threshold {
        *armed = true;
    }
    if *armed && idle_ms >= threshold && !locked {
        *armed = false;
        return true;
    }
    false
}

fn monitor_session(config: &Config, stop: &AtomicBool, armed: &mut bool) -> Result<()> {
    let connection = Connection::session()?;
    let (idle, lock) = proxies(&connection)?;
    let executable = env::current_exe()?;
    while !stop.load(Ordering::Relaxed) {
        let idle_ms: u64 = idle.call("GetIdletime", &())?;
        let locked: bool = lock.call("GetActive", &())?;
        if should_launch(armed, idle_ms, config.idle_seconds, locked) {
            println!("Starting Ambient Photos after {}s idle", idle_ms / 1000);
            let mut child = Command::new(&executable)
                .args(&config.photo_directories)
                .spawn()?;
            loop {
                if stop.load(Ordering::Relaxed) {
                    stop_child(&mut child);
                    return Ok(());
                }
                if child.try_wait()?.is_some() {
                    break;
                }
                match lock.call::<_, _, bool>("GetActive", &()) {
                    Ok(true) => {
                        stop_child(&mut child);
                        break;
                    }
                    Ok(false) => {}
                    Err(error) => eprintln!("GNOME lock check failed: {error}"),
                }
                thread::sleep(POLL);
            }
            println!("Display closed; waiting for new user activity");
        }
        thread::sleep(POLL);
    }
    Ok(())
}

fn monitor() -> Result<()> {
    let locations = paths()?;
    let config: Config = serde_json::from_slice(&fs::read(&locations.config)?)?;
    if config.idle_seconds == 0 || config.photo_directories.is_empty() {
        return Err("invalid GNOME idle configuration".into());
    }
    let runtime = env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    let lock_path = PathBuf::from(runtime).join("ambient-photos-idle.lock");
    let lock = File::create(lock_path)?;
    lock.try_lock()
        .map_err(|_| "another Ambient Photos idle watcher is running")?;
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    println!("Waiting for {}s of GNOME inactivity", config.idle_seconds);
    let mut armed = true;
    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) {
        if let Err(error) = monitor_session(&config, &stop, &mut armed) {
            let message = error.to_string();
            if message != last_error {
                eprintln!("GNOME idle monitor unavailable: {message}");
                last_error = message;
            }
            thread::sleep(POLL);
        }
    }
    drop(lock);
    Ok(())
}

pub fn run(args: Vec<String>) -> Result<()> {
    match args.as_slice() {
        [command, rest @ ..] if command == "install" => install(rest),
        [command] if command == "status" => status(),
        [command] if command == "uninstall" => uninstall(),
        [command] if command == "check" => check(),
        [command] if command == "run" => monitor(),
        _ => Err("use: ambient-screensaver gnome install [--idle-seconds N] PHOTO_DIR... | status | check | run | uninstall".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_is_gnome_only_and_escapes_its_command() {
        let entry = desktop_entry(Path::new("/some path/ambient$photos%")).unwrap();
        assert!(entry.contains("OnlyShowIn=GNOME;"));
        assert!(entry.contains("/some path/ambient\\\\$photos%%"));
        assert!(entry.contains(MANAGED_MARKER));
    }

    #[test]
    fn idle_period_launches_once_after_unlock() {
        let mut armed = true;
        assert!(!should_launch(&mut armed, 119_000, 120, false));
        assert!(!should_launch(&mut armed, 121_000, 120, true));
        assert!(should_launch(&mut armed, 121_000, 120, false));
        assert!(!should_launch(&mut armed, 180_000, 120, false));
        assert!(!should_launch(&mut armed, 0, 120, false));
        assert!(should_launch(&mut armed, 120_000, 120, false));
    }
}
