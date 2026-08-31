use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use clap::ValueEnum;

use crate::error::Error;

const SERVICE_NAME: &str = "mprisence.service";
const DESKTOP_FILE_NAME: &str = "mprisence.desktop";
const MANAGED_MARKER: &str = "Managed by `mprisence autostart`";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Method {
    #[default]
    Auto,
    Systemd,
    Desktop,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendStatus {
    pub available: bool,
    pub installed: bool,
    pub enabled: bool,
    pub active: Option<bool>,
    pub path: Option<PathBuf>,
    pub managed: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub systemd: BackendStatus,
    pub desktop: BackendStatus,
}

impl Status {
    pub fn enabled(&self) -> bool {
        self.systemd.enabled || self.desktop.enabled
    }

    pub fn has_conflict(&self) -> bool {
        self.systemd.enabled && self.desktop.enabled
    }
}

pub fn status() -> Status {
    Status {
        systemd: systemd_status(),
        desktop: desktop_status(),
    }
}

pub fn enable(method: Method) -> Result<Status, Error> {
    let before = status();
    let selected = match method {
        Method::Auto if before.systemd.available => Method::Systemd,
        Method::Auto => Method::Desktop,
        explicit => explicit,
    };

    match selected {
        Method::Systemd => {
            if !before.systemd.available {
                return Err(autostart_error(
                    "the systemd user manager is unavailable; use `mprisence autostart enable --method desktop` for desktop-login autostart",
                ));
            }
            prevent_conflicting_method(&before.desktop, "desktop-login")?;
            enable_systemd()?;
        }
        Method::Desktop => {
            prevent_conflicting_method(&before.systemd, "systemd")?;
            enable_desktop()?;
        }
        Method::Auto => unreachable!(),
    }

    Ok(status())
}

pub fn disable() -> Result<Status, Error> {
    let before = status();
    let mut failures = Vec::new();

    if before.systemd.available && (before.systemd.installed || before.systemd.active == Some(true))
    {
        if let Err(err) = disable_systemd() {
            failures.push(err.to_string());
        }
    }

    if before.desktop.installed {
        if let Err(err) = disable_desktop(&before.desktop) {
            failures.push(err.to_string());
        }
    }

    if failures.is_empty() {
        Ok(status())
    } else {
        Err(autostart_error(failures.join("; ")))
    }
}

pub fn restart() -> Result<Status, Error> {
    let current = status();
    if !current.systemd.available {
        return Err(autostart_error(
            "restart is available only for the systemd autostart method",
        ));
    }
    if !current.systemd.installed {
        return Err(autostart_error(
            "the mprisence systemd user service is not installed",
        ));
    }
    checked_systemctl(&["restart", SERVICE_NAME])?;
    Ok(status())
}

pub fn print_status(current: &Status) {
    println!("mprisence autostart\n");

    if current.has_conflict() {
        println!("Autostart : conflict (systemd and desktop login are both enabled)");
    } else {
        println!(
            "Autostart : {}",
            if current.enabled() {
                "enabled"
            } else {
                "disabled"
            }
        );
    }

    if current.systemd.available {
        let enabled = if current.systemd.enabled {
            "enabled"
        } else if current.systemd.installed {
            "disabled"
        } else {
            "not installed"
        };
        let runtime = match current.systemd.active {
            Some(true) => ", running",
            Some(false) => ", stopped",
            None => "",
        };
        println!("Systemd   : {enabled}{runtime}");
        if let Some(path) = &current.systemd.path {
            println!("Unit      : {}", path.display());
        }
    } else {
        println!("Systemd   : unavailable");
        if let Some(detail) = &current.systemd.detail {
            println!("             {detail}");
        }
    }

    if current.desktop.installed || current.desktop.enabled {
        println!(
            "Desktop   : {}",
            if current.desktop.enabled {
                "enabled at login"
            } else {
                "disabled"
            }
        );
        if let Some(path) = &current.desktop.path {
            println!("Entry     : {}", path.display());
        }
    }

    println!();
    if current.enabled() {
        println!("Disable with: mprisence autostart disable");
    } else {
        println!("Enable with:  mprisence autostart enable");
    }
}

fn prevent_conflicting_method(other: &BackendStatus, name: &str) -> Result<(), Error> {
    if !other.enabled {
        return Ok(());
    }
    let location = other
        .path
        .as_ref()
        .map(|path| format!(" at {}", path.display()))
        .unwrap_or_default();
    Err(autostart_error(format!(
        "{name} autostart is already enabled{location}; run `mprisence autostart disable` before switching methods"
    )))
}

fn systemd_status() -> BackendStatus {
    let availability = systemctl(&["show-environment"]);
    let available = matches!(&availability, Ok(output) if output.status.success());
    if !available {
        let detail = match availability {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                "systemctl was not found".to_string()
            }
            Err(err) => err.to_string(),
            Ok(output) => command_message(&output),
        };
        return BackendStatus {
            detail: Some(detail),
            ..BackendStatus::default()
        };
    }

    let load_state = systemd_property("LoadState");
    let installed = load_state
        .as_deref()
        .is_some_and(|state| state != "not-found");
    let enabled_state = systemctl_text(&["is-enabled", SERVICE_NAME]);
    let enabled = matches!(
        enabled_state.as_deref(),
        Some("enabled" | "enabled-runtime")
    );
    let active_state = systemctl_text(&["is-active", SERVICE_NAME]);
    let active = Some(matches!(
        active_state.as_deref(),
        Some("active" | "reloading" | "refreshing")
    ));
    let path = systemd_property("FragmentPath")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let managed = path.as_ref().is_some_and(|path| file_is_managed(path));

    BackendStatus {
        available,
        installed,
        enabled,
        active,
        path,
        managed,
        detail: load_state,
    }
}

fn desktop_status() -> BackendStatus {
    let user_path = desktop_user_path();
    let system_path = desktop_system_paths()
        .into_iter()
        .find(|path| path.exists());
    let path = if user_path.exists() {
        Some(user_path)
    } else {
        system_path
    };
    let installed = path.is_some();
    let content = path.as_ref().and_then(|path| fs::read_to_string(path).ok());
    let enabled = content.as_deref().is_some_and(|text| !desktop_hidden(text));
    let managed = content
        .as_deref()
        .is_some_and(|text| text.contains(MANAGED_MARKER));

    BackendStatus {
        available: true,
        installed,
        enabled,
        active: None,
        path,
        managed,
        detail: None,
    }
}

fn enable_systemd() -> Result<(), Error> {
    let user_path = systemd_user_path();
    let mut current = systemd_status();

    if !current.installed {
        checked_systemctl(&["daemon-reload"])?;
        current = systemd_status();
    }

    let refresh_user_unit = current.path.as_ref() == Some(&user_path)
        && (current.managed || file_is_legacy_unit(&user_path));
    if !current.installed || refresh_user_unit {
        let executable = stable_executable()?;
        let unit = render_systemd_unit(&executable)?;
        write_atomic(&user_path, &unit)?;
        checked_systemctl(&["daemon-reload"])?;
    }

    checked_systemctl(&["enable", "--now", SERVICE_NAME])
}

fn disable_systemd() -> Result<(), Error> {
    checked_systemctl(&["disable", "--now", SERVICE_NAME])
}

fn enable_desktop() -> Result<(), Error> {
    let user_path = desktop_user_path();
    if user_path.exists() && !file_is_managed(&user_path) {
        let content = fs::read_to_string(&user_path)?;
        return write_atomic(&user_path, &set_desktop_hidden(&content, false));
    }

    if !user_path.exists() && desktop_system_paths().into_iter().any(|path| path.exists()) {
        return Ok(());
    }

    let executable = stable_executable()?;
    let entry = render_desktop_entry(&executable)?;
    write_atomic(&user_path, &entry)
}

fn disable_desktop(current: &BackendStatus) -> Result<(), Error> {
    let user_path = desktop_user_path();
    let system_entry_exists = desktop_system_paths().into_iter().any(|path| path.exists());

    if current.path.as_ref() == Some(&user_path) && current.managed && !system_entry_exists {
        fs::remove_file(user_path)?;
        return Ok(());
    }

    let disabled = if user_path.exists() && !current.managed {
        set_desktop_hidden(&fs::read_to_string(&user_path)?, true)
    } else {
        format!(
            "# {MANAGED_MARKER}\n[Desktop Entry]\nType=Application\nName=mprisence\nHidden=true\n"
        )
    };
    write_atomic(&user_path, &disabled)
}

fn systemd_user_path() -> PathBuf {
    config_home().join("systemd/user").join(SERVICE_NAME)
}

fn desktop_user_path() -> PathBuf {
    config_home().join("autostart").join(DESKTOP_FILE_NAME)
}

fn desktop_system_paths() -> Vec<PathBuf> {
    env::var_os("XDG_CONFIG_DIRS")
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .filter(|paths| !paths.is_empty())
        .unwrap_or_else(|| vec![PathBuf::from("/etc/xdg")])
        .into_iter()
        .map(|base| base.join("autostart").join(DESKTOP_FILE_NAME))
        .collect()
}

fn config_home() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"))
}

fn stable_executable() -> Result<PathBuf, Error> {
    let current = env::current_exe()?;
    let canonical_current = fs::canonicalize(&current).unwrap_or_else(|_| current.clone());

    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join("mprisence");
            if !candidate.is_file() {
                continue;
            }
            if fs::canonicalize(&candidate).ok().as_ref() == Some(&canonical_current) {
                return Ok(candidate);
            }
        }
    }

    Ok(current)
}

fn render_systemd_unit(executable: &Path) -> Result<String, Error> {
    let executable = quote_systemd_path(executable)?;
    Ok(format!(
        "# {MANAGED_MARKER}\n[Unit]\nDescription=Discord Rich Presence for MPRIS media players\n\n[Service]\nType=simple\nExecStart={executable}\nRestart=on-failure\nRestartSec=10\nEnvironment=RUST_LOG=info\nEnvironment=RUST_BACKTRACE=1\n\n[Install]\nWantedBy=default.target\n"
    ))
}

fn render_desktop_entry(executable: &Path) -> Result<String, Error> {
    let executable = quote_desktop_exec(executable)?;
    Ok(format!(
        "# {MANAGED_MARKER}\n[Desktop Entry]\nType=Application\nName=mprisence\nComment=Discord Rich Presence for media players\nExec={executable}\nTerminal=false\nHidden=false\n"
    ))
}

fn quote_systemd_path(path: &Path) -> Result<String, Error> {
    let value = path_to_str(path)?;
    if value.contains(['\n', '\r']) {
        return Err(autostart_error(
            "the mprisence executable path contains a newline",
        ));
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    Ok(format!("\"{escaped}\""))
}

fn quote_desktop_exec(path: &Path) -> Result<String, Error> {
    let value = path_to_str(path)?;
    if value.contains(['\n', '\r']) {
        return Err(autostart_error(
            "the mprisence executable path contains a newline",
        ));
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    Ok(format!("\"{escaped}\""))
}

fn path_to_str(path: &Path) -> Result<&str, Error> {
    path.to_str()
        .ok_or_else(|| autostart_error("the mprisence executable path is not valid UTF-8"))
}

fn write_atomic(path: &Path, content: &str) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| autostart_error("autostart path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp_path = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp_path, content)?;
    if let Err(err) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err.into());
    }
    Ok(())
}

fn systemctl(args: &[&str]) -> io::Result<Output> {
    Command::new("systemctl").arg("--user").args(args).output()
}

fn checked_systemctl(args: &[&str]) -> Result<(), Error> {
    let output = systemctl(args)?;
    if output.status.success() {
        return Ok(());
    }
    Err(autostart_error(format!(
        "`systemctl --user {}` failed: {}",
        args.join(" "),
        command_message(&output)
    )))
}

fn systemctl_text(args: &[&str]) -> Option<String> {
    let output = systemctl(args).ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

fn systemd_property(name: &str) -> Option<String> {
    systemctl_text(&["show", SERVICE_NAME, "--property", name, "--value"])
}

fn command_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("exited with {}", output.status)
}

fn file_is_managed(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| text.contains(MANAGED_MARKER))
        .unwrap_or(false)
}

fn file_is_legacy_unit(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .any(|line| line.trim() == "ExecStart=%h/.cargo/bin/mprisence")
        })
        .unwrap_or(false)
}

fn desktop_hidden(content: &str) -> bool {
    content.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        key.trim().eq_ignore_ascii_case("Hidden") && value.trim().eq_ignore_ascii_case("true")
    })
}

fn set_desktop_hidden(content: &str, hidden: bool) -> String {
    let value = if hidden { "true" } else { "false" };
    let mut found = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let is_hidden = line
            .split_once('=')
            .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case("Hidden"));
        if is_hidden {
            if !found {
                lines.push(format!("Hidden={value}"));
                found = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("Hidden={value}"));
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

fn autostart_error(message: impl Into<String>) -> Error {
    Error::Autostart(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_systemd_unit_quotes_paths_and_specifiers() {
        let unit = render_systemd_unit(Path::new("/home/Test User/bin/mprisence%dev")).unwrap();
        assert!(unit.contains("ExecStart=\"/home/Test User/bin/mprisence%%dev\""));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn generated_desktop_entry_quotes_paths() {
        let entry = render_desktop_entry(Path::new("/home/Test User/bin/mprisence")).unwrap();
        assert!(entry.contains("Exec=\"/home/Test User/bin/mprisence\""));
        assert!(entry.contains("Terminal=false"));
    }

    #[test]
    fn desktop_hidden_is_case_insensitive() {
        assert!(desktop_hidden("[Desktop Entry]\nhidden = TRUE\n"));
        assert!(!desktop_hidden("[Desktop Entry]\nHidden=false\n"));
    }

    #[test]
    fn desktop_toggle_preserves_custom_entry() {
        let original = "[Desktop Entry]\nName=Custom\nExec=/custom/mprisence\nHidden=true\n";
        let enabled = set_desktop_hidden(original, false);
        assert!(enabled.contains("Name=Custom"));
        assert!(enabled.contains("Exec=/custom/mprisence"));
        assert!(enabled.contains("Hidden=false"));
    }

    #[test]
    fn legacy_unit_detection_is_specific_to_the_shipped_cargo_path() {
        let path =
            env::temp_dir().join(format!("mprisence-legacy-unit-test-{}", std::process::id()));
        fs::write(&path, "[Service]\nExecStart=%h/.cargo/bin/mprisence\n").unwrap();
        assert!(file_is_legacy_unit(&path));
        fs::write(&path, "[Service]\nExecStart=/custom/mprisence\n").unwrap();
        assert!(!file_is_legacy_unit(&path));
        fs::remove_file(path).unwrap();
    }
}
