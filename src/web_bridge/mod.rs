mod active_source;
pub mod mpris;
mod native_messaging;
pub mod protocol;

use active_source::SourceRegistry;
use log::{debug, error, info, trace, warn};
use mpris::{MprisPublisher, PlayerManager, TaggedCommand};
use native_messaging::{read_message, send_message};
use protocol::{BridgeMessage, ExtMessage, SourceState};
use std::{
    io::stdout,
    path::{Path, PathBuf},
};
use tokio::io::BufReader;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

pub const EXTENSION_ID: &str = "mprisence-bridge@lazykern.foo";
pub const CHROME_EXTENSION_ID: &str = "pphdmbejbipjlocngoefnmjoijcbdejf";
pub const HOST_NAME: &str = "mprisence.web.bridge";
const HOST_MANIFEST_FILENAME: &str = "mprisence.web.bridge.json";
const BRIDGE_LOG_PATH: &str = "/tmp/bridge-stderr.log";

pub fn init_host_logging() {
    let _ = std::fs::create_dir_all("/tmp");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(BRIDGE_LOG_PATH)
        .expect("open bridge log file");

    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .target(env_logger::Target::Pipe(Box::new(log_file)))
        .try_init();
}

pub fn looks_like_native_host_invocation(args: &[String]) -> bool {
    !args.is_empty()
        && args.iter().any(|arg| {
            arg == EXTENSION_ID
                || arg == CHROME_EXTENSION_ID
                || arg.starts_with("chrome-extension://")
                || arg.ends_with(HOST_MANIFEST_FILENAME)
                || arg.contains(&format!("/{HOST_MANIFEST_FILENAME}"))
        })
}

pub fn is_explicit_host_command(args: &[String]) -> bool {
    matches!(args, [web, host, ..] if web == "web" && host == "host")
}

pub async fn run_host() {
    info!("Starting mprisence web bridge (multi-player)");

    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async {
            run_host_inner().await;
        })
        .await;
}

async fn run_host_inner() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<TaggedCommand>(64);

    let mut registry = SourceRegistry::new();
    let mut players = PlayerManager::new();

    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin);

    let mut heartbeat_timer = interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            msg_result = read_message(&mut stdin_reader) => {
                match msg_result {
                    Ok(Some(bytes)) => {
                        match serde_json::from_slice::<ExtMessage>(&bytes) {
                            Ok(ext_msg) => {
                                trace!("← ext: {}", String::from_utf8_lossy(
                                    &bytes[..bytes.len().min(200)]));
                                handle_extension_message(ext_msg, &mut registry,
                                    &mut players, &mut stdout(), &cmd_tx).await;
                            }
                            Err(e) => warn!("Failed to parse: {e}"),
                        }
                    }
                    Ok(None) => { info!("Browser disconnected (EOF)"); break; }
                    Err(e) => { error!("stdin error: {e}"); break; }
                }
            }

            Some((source_id, cmd)) = cmd_rx.recv() => {
                trace!("← MPRIS cmd from {source_id}: {cmd:?}");
                let msg = bridge_command_for(&source_id, &cmd);
                if let Err(e) = send_message(&mut stdout(), &msg) {
                    warn!("Failed to send command: {e}");
                }
            }

            _ = heartbeat_timer.tick() => {
                let removed = registry.prune_stale();
                for id in &removed {
                    players.remove_player(id);
                }
            }
        }
    }

    info!("Bridge shutting down");
}

fn bridge_command_for(source_id: &str, cmd: &mpris::MprisCommand) -> BridgeMessage {
    use mpris::MprisCommand;
    let command = match cmd {
        MprisCommand::PlayPause => protocol::CommandKind::PlayPause,
        MprisCommand::Play => protocol::CommandKind::Play,
        MprisCommand::Pause => protocol::CommandKind::Pause,
        MprisCommand::Next => protocol::CommandKind::Next,
        MprisCommand::Previous => protocol::CommandKind::Previous,
        MprisCommand::Seek(_) => protocol::CommandKind::Seek,
        MprisCommand::SetPosition(_) => protocol::CommandKind::SetPosition,
        MprisCommand::Stop => protocol::CommandKind::Pause,
    };
    let position_ms = match cmd {
        MprisCommand::SetPosition(us) if *us >= 0 => Some((*us / 1000) as u64),
        _ => None,
    };
    BridgeMessage::Command {
        source_id: source_id.to_string(),
        command,
        position_ms,
    }
}

async fn handle_extension_message(
    msg: ExtMessage,
    registry: &mut SourceRegistry,
    players: &mut PlayerManager,
    stdout: &mut impl std::io::Write,
    cmd_tx: &mpsc::Sender<TaggedCommand>,
) {
    match msg {
        ExtMessage::Hello {
            browser,
            extension_version,
            protocol,
            git_sha,
            extension_fingerprint,
        } => {
            info!("Extension connected: {browser:?} v{extension_version}");

            if protocol != protocol::PROTOCOL_VERSION {
                warn!(
                    "Protocol mismatch: extension protocol={protocol}, bridge protocol={}",
                    protocol::PROTOCOL_VERSION
                );
            }
            if let Some(sha) = &git_sha {
                info!("Extension git SHA: {sha}");
            }
            if let Some(fp) = &extension_fingerprint {
                info!("Extension fingerprint: {fp}");
            }

            let hello = BridgeMessage::Hello {
                bridge_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol: protocol::PROTOCOL_VERSION,
                git_sha: option_env!("GIT_SHA").map(str::to_string),
            };
            if let Err(e) = send_message(stdout, &hello) {
                warn!("Failed to send hello: {e}");
            }
        }

        ExtMessage::Update {
            source_id,
            url,
            origin,
            site,
            playback,
            metadata,
            capabilities,
            canonical_url,
        } => {
            let site_for_player = site.clone();
            let state = SourceState {
                source_id: source_id.clone(),
                url,
                origin,
                site,
                playback,
                metadata,
                capabilities,
                canonical_url,
                last_seen: std::time::Instant::now(),
            };

            registry.upsert(state);

            let had_player_before = players.has_player(&source_id);
            let current_count = players.player_count();
            debug!("ensuring player for {source_id} site={site_for_player} (had={had_player_before} total_players={current_count})");
            match players
                .ensure_player(&source_id, &site_for_player, cmd_tx)
                .await
            {
                Some(publisher) => {
                    if !had_player_before {
                        info!(
                            "PlayerManager: new player created for {source_id}, bus={}",
                            publisher.bus_name()
                        );
                    }
                    if let Some(state) = registry.get(&source_id) {
                        publisher.publish(Some(state)).await;
                    }
                }
                None => {
                    warn!("No MPRIS player for {source_id} (ensure_player returned None)");
                }
            }
        }

        ExtMessage::Remove { source_id } => {
            debug!("Source removed: {source_id}");
            registry.remove(&source_id);
            players.remove_player(&source_id);
        }
    }
}

pub async fn install(browsers: Vec<String>) {
    let requested = |name: &str| browsers.is_empty() || browsers.iter().any(|b| b == name);
    let binary = std::env::current_exe().expect("could not resolve bridge binary path");

    if requested("firefox") {
        install_firefox_manifest(&binary);
    }
    if requested("chromium") {
        install_chromium_manifest(&binary);
    }
    println!("Done. You may need to restart browser.");
}

fn manifest_dir_firefox() -> PathBuf {
    let home = std::env::var_os("HOME").expect("$HOME not set");
    PathBuf::from(home).join(".mozilla/native-messaging-hosts")
}

fn manifest_dir_chromium() -> PathBuf {
    let home = std::env::var_os("HOME").expect("$HOME not set");
    PathBuf::from(home).join(".config/chromium/NativeMessagingHosts")
}

fn manifest_dir_google_chrome() -> PathBuf {
    let home = std::env::var_os("HOME").expect("$HOME not set");
    PathBuf::from(home).join(".config/google-chrome/NativeMessagingHosts")
}

fn chromium_manifest_targets() -> Vec<(String, PathBuf)> {
    let home = std::env::var_os("HOME").expect("$HOME not set");
    let config_root = PathBuf::from(home).join(".config");

    let mut targets = std::collections::BTreeMap::<String, PathBuf>::new();
    targets.insert("Chromium".into(), manifest_dir_chromium());
    targets.insert("Google Chrome".into(), manifest_dir_google_chrome());

    if let Ok(entries) = std::fs::read_dir(&config_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let is_chrome_family = name == "chromium"
                || name.starts_with("chromium-")
                || name == "google-chrome"
                || name.starts_with("google-chrome-");
            if !is_chrome_family {
                continue;
            }

            let label = if name == "chromium" {
                "Chromium".to_string()
            } else if name == "google-chrome" {
                "Google Chrome".to_string()
            } else {
                format!("Chrome profile root {name}")
            };
            targets.insert(label, path.join("NativeMessagingHosts"));
        }
    }

    targets.into_iter().collect()
}

fn install_firefox_manifest(binary: &Path) {
    let dir = manifest_dir_firefox();
    let path = dir.join(HOST_MANIFEST_FILENAME);

    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "mprisence — sends browser media to MPRIS",
        "path": binary.to_str().expect("binary path is not UTF-8"),
        "type": "stdio",
        "allowed_extensions": [EXTENSION_ID],
    });

    std::fs::create_dir_all(&dir).expect("create native-messaging-hosts dir");
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap())
        .expect("write Firefox native messaging manifest");

    println!("✓ Firefox: {}", path.display());
}

fn install_chromium_manifest(binary: &Path) {
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "mprisence — sends browser media to MPRIS",
        "path": binary.to_str().expect("binary path is not UTF-8"),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{CHROME_EXTENSION_ID}/")],
    });

    for (browser, dir) in chromium_manifest_targets() {
        let path = dir.join(HOST_MANIFEST_FILENAME);
        std::fs::create_dir_all(&dir).expect("create NativeMessagingHosts dir");
        std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap())
            .expect("write Chromium native messaging manifest");
        println!("✓ {browser}: {}", path.display());
    }
}

pub async fn uninstall(browsers: Vec<String>) {
    let requested = |name: &str| browsers.is_empty() || browsers.iter().any(|b| b == name);

    if requested("firefox") {
        let path = manifest_dir_firefox().join(HOST_MANIFEST_FILENAME);
        if path.exists() {
            std::fs::remove_file(&path).ok();
            println!("✗ Removed Firefox: {}", path.display());
        } else {
            println!("  Firefox manifest not found: {}", path.display());
        }
    }
    if requested("chromium") {
        for (browser, dir) in chromium_manifest_targets() {
            let path = dir.join(HOST_MANIFEST_FILENAME);
            if path.exists() {
                std::fs::remove_file(&path).ok();
                println!("✗ Removed {browser}: {}", path.display());
            } else {
                println!("  {browser} manifest not found: {}", path.display());
            }
        }
    }
}

/// Check native-host manifests. Returns number of issues found.
pub fn check_native_host_manifests() -> usize {
    let mut issues = 0;

    let binary = std::env::current_exe().ok();
    match &binary {
        Some(p) => println!("✓ Binary: {}", p.display()),
        None => {
            issues += 1;
            println!("✗ Binary: could not resolve");
        }
    }

    let mut targets = vec![("Firefox".to_string(), manifest_dir_firefox())];
    targets.extend(chromium_manifest_targets());

    for (browser, dir) in targets {
        let manifest_path = dir.join(HOST_MANIFEST_FILENAME);
        if manifest_path.exists() {
            match std::fs::read_to_string(&manifest_path) {
                Ok(content) => {
                    if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                        let path_ok = manifest
                            .get("path")
                            .and_then(|p| p.as_str())
                            .map(|p| Path::new(p).exists())
                            .unwrap_or(false);
                        if path_ok {
                            println!(
                                "✓ {browser} manifest: {} (binary OK)",
                                manifest_path.display()
                            );
                        } else {
                            issues += 1;
                            println!(
                                "⚠ {browser} manifest: {} (binary missing/stale)",
                                manifest_path.display()
                            );
                        }
                    } else {
                        issues += 1;
                        println!(
                            "⚠ {browser} manifest: {} (invalid JSON)",
                            manifest_path.display()
                        );
                    }
                }
                Err(e) => {
                    issues += 1;
                    println!(
                        "✗ {browser} manifest: {} (read error: {e})",
                        manifest_path.display()
                    );
                }
            }
        } else {
            issues += 1;
            println!(
                "✗ {browser} manifest: not found at {}",
                manifest_path.display()
            );
        }
    }

    issues
}

pub fn check_dbus_session() -> bool {
    std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok()
}

pub async fn doctor() {
    println!("🧑‍⚕️ mprisence web doctor\n");

    check_native_host_manifests();
    if check_dbus_session() {
        println!("✓ D-Bus session bus: available");
    } else {
        println!("✗ D-Bus session bus: DBUS_SESSION_BUS_ADDRESS not set");
    }
}

pub async fn debug_fake_player(mpris_name: String) {
    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async move {
            debug_fake_player_inner(mpris_name).await;
            futures::future::pending::<()>().await;
        })
        .await;
}

async fn debug_fake_player_inner(mpris_name: String) {
    info!("Starting fake test player: {mpris_name}");

    let (cmd_tx, mut _cmd_rx) = mpsc::channel::<TaggedCommand>(64);

    let publisher = MprisPublisher::new(&mpris_name, "debug:fake:1", cmd_tx)
        .await
        .unwrap();
    let run_task = publisher.run_task();
    let _handle = tokio::task::spawn_local(run_task);

    use protocol::*;
    let fake_source = SourceState {
        source_id: "debug:fake:1".into(),
        url: "https://music.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        origin: "https://music.youtube.com".into(),
        site: "youtube_music".into(),
        playback: PlaybackState {
            status: Status::Playing,
            position_ms: 42000,
            duration_ms: 212000,
        },
        metadata: MediaMetadata {
            title: Some("Never Gonna Give You Up".into()),
            artist: vec!["Rick Astley".into()],
            album: Some("Whenever You Need Somebody".into()),
            album_artist: vec!["Rick Astley".into()],
            art_url: Some(
                "https://upload.wikimedia.org/wikipedia/en/5/53/Rick_Astley_-_Never_Gonna_Give_You_Up.png"
                    .into(),
            ),
            track_id: Some("debug:rickroll".into()),
        },
        capabilities: Capabilities {
            play_pause: true,
            next: true,
            previous: true,
            seek: true,
            set_position: true,
        },
        canonical_url: Some("https://music.youtube.com/watch?v=dQw4w9WgXcQ".into()),
        last_seen: std::time::Instant::now(),
    };

    info!("Publishing fake player...");
    publisher.publish(Some(&fake_source)).await;
    info!("Fake player published! Check with: playerctl metadata");
    info!("Running...");
}
