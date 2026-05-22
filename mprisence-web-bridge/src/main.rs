mod active_source;
mod mpris;
mod native_messaging;
mod protocol;

use active_source::SourceRegistry;
use clap::{Parser, Subcommand};
use log::{debug, error, info, trace, warn};
use mpris::{MprisPublisher, PlayerManager, TaggedCommand};
use native_messaging::{read_message, send_message};
use protocol::{BridgeMessage, ExtMessage, SourceState};
use std::io::stdout;
use tokio::io::BufReader;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

/// Firefox extension ID — must match manifest.firefox.json
const EXTENSION_ID: &str = "mprisence-bridge@lazykern.github.io";
/// Native messaging host name — must match extension/src/utils/native-messaging.ts
const HOST_NAME: &str = "mprisence.web.bridge";

/// Firefox/Chromium native messaging protocol passes extra positional args:
///   1. manifest path (Firefox)
///   2. caller origin (Firefox) / extension ID (Chromium)
/// We must accept these without error so Clap doesn't reject them.
#[derive(Parser)]
#[command(name = "mprisence-web-bridge")]
#[command(about = "Browser → MPRIS bridge for mprisence")]
#[command(version)]
#[command(trailing_var_arg = true)]
struct Cli {
    /// Extra native-messaging host args passed by the browser (ignored).
    #[arg(hide = true)]
    native_args: Vec<String>,

    #[command(subcommand)]
    command: Option<BridgeCommand>,
}

#[derive(Subcommand)]
enum BridgeCommand {
    /// Run the bridge (native messaging host).
    #[command(hide = true)]
    Run {
        #[arg(long, default_value = "web")]
        _mpris_name: String,
    },
    /// Install native messaging host manifests for detected browsers.
    Install {
        /// Only install for specific browser(s): firefox, chromium
        #[arg(short, long)]
        browser: Vec<String>,
    },
    /// Remove native messaging host manifests.
    Uninstall {
        /// Only remove for specific browser(s): firefox, chromium
        #[arg(short, long)]
        browser: Vec<String>,
    },
    /// Check native messaging setup.
    Doctor,
    /// Run with a fake test source (for development).
    #[command(hide = true)]
    DebugFakePlayer {
        #[arg(long, default_value = "web_debug")]
        mpris_name: String,
    },
}

/// Single-threaded tokio runtime — mpris_server::Player returns !Send
#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();

    let cli = Cli::parse();

    match cli.command {
        Some(BridgeCommand::Run { .. }) => run_bridge().await,
        Some(BridgeCommand::Install { browser }) => cmd_install(browser).await,
        Some(BridgeCommand::Uninstall { browser }) => cmd_uninstall(browser).await,
        Some(BridgeCommand::Doctor) => cmd_doctor().await,
        Some(BridgeCommand::DebugFakePlayer { mpris_name }) => debug_fake_player(mpris_name).await,
        None => run_bridge().await,
    }
}

// ─── Run ──────────────────────────────────────────────────────────

async fn run_bridge() {
    info!("Starting mprisence-web-bridge (multi-player)");

    // Wrap in LocalSet so we can spawn_local() for each player's run task
    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async {
            run_bridge_inner().await;
        })
        .await;
}

async fn run_bridge_inner() {
    // Shared command channel: (source_id, command) pairs from all MPRIS players
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
                // Forward command to the extension (targeting specific source)
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
                // Run selection to update active_source_id
                registry.select_active();
                // Publish current state for each player
                for (source_id, state) in registry.sources() {
                    if let Some(publisher) = players.get(source_id) {
                        publisher.publish(Some(state)).await;
                    }
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
        // SetPosition is absolute. Seek is relative (and may be negative), but
        // the extension protocol only carries absolute position_ms, so don't
        // send bogus wrapped offsets for Seek.
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
        ExtMessage::Hello { browser, extension_version, protocol, git_sha } => {
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

            let hello = BridgeMessage::Hello {
                bridge_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol: protocol::PROTOCOL_VERSION,
                git_sha: Some(env!("GIT_SHA").to_string()),
            };
            if let Err(e) = send_message(stdout, &hello) {
                warn!("Failed to send hello: {e}");
            }
        }

        ExtMessage::Update { source_id, url, origin, site, playback, metadata, capabilities, confidence, canonical_url } => {
            let site_for_player = site.clone();
            let state = SourceState {
                source_id: source_id.clone(),
                url, origin, site,
                playback, metadata, capabilities, confidence,
                canonical_url,
                last_seen: std::time::Instant::now(),
            };
            registry.upsert(state);

            // Ensure this source has an MPRIS player, then publish.
            if let Some(publisher) = players.ensure_player(&source_id, &site_for_player, cmd_tx).await {
                if let Some(state) = registry.get(&source_id) {
                    publisher.publish(Some(state)).await;
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

// ─── Install / Uninstall / Doctor ────────────────────────────────

async fn cmd_install(browsers: Vec<String>) {
    let requested = |name: &str| browsers.is_empty() || browsers.iter().any(|b| b == name);
    let binary = std::env::current_exe()
        .expect("could not resolve bridge binary path");

    if requested("firefox") {
        install_firefox_manifest(&binary);
    }
    if requested("chromium") {
        install_chromium_manifest(&binary);
    }
    println!("Done. You may need to restart the browser.");
}

fn manifest_dir_firefox() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").expect("$HOME not set");
    std::path::PathBuf::from(home).join(".mozilla/native-messaging-hosts")
}

fn manifest_dir_chromium() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").expect("$HOME not set");
    std::path::PathBuf::from(home).join(".config/chromium/NativeMessagingHosts")
}

fn install_firefox_manifest(binary: &std::path::Path) {
    let dir = manifest_dir_firefox();
    let path = dir.join(format!("{HOST_NAME}.json"));

    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "mprisence-web-bridge — sends browser media to MPRIS",
        "path": binary.to_str().expect("binary path is not UTF-8"),
        "type": "stdio",
        "allowed_extensions": [EXTENSION_ID],
    });

    std::fs::create_dir_all(&dir).expect("create native-messaging-hosts dir");
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap())
        .expect("write Firefox native messaging manifest");

    println!("✓ Firefox: {}", path.display());
}

fn install_chromium_manifest(binary: &std::path::Path) {
    let dir = manifest_dir_chromium();
    let path = dir.join(format!("{HOST_NAME}.json"));

    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "mprisence-web-bridge — sends browser media to MPRIS",
        "path": binary.to_str().expect("binary path is not UTF-8"),
        "type": "stdio",
        "allowed_extensions": [EXTENSION_ID],
    });

    std::fs::create_dir_all(&dir).expect("create NativeMessagingHosts dir");
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap())
        .expect("write Chromium native messaging manifest");

    println!("✓ Chromium: {}", path.display());
}

async fn cmd_uninstall(browsers: Vec<String>) {
    let requested = |name: &str| browsers.is_empty() || browsers.iter().any(|b| b == name);

    if requested("firefox") {
        let path = manifest_dir_firefox().join(format!("{HOST_NAME}.json"));
        if path.exists() {
            std::fs::remove_file(&path).ok();
            println!("✗ Removed Firefox: {}", path.display());
        } else {
            println!("  Firefox manifest not found: {}", path.display());
        }
    }
    if requested("chromium") {
        let path = manifest_dir_chromium().join(format!("{HOST_NAME}.json"));
        if path.exists() {
            std::fs::remove_file(&path).ok();
            println!("✗ Removed Chromium: {}", path.display());
        } else {
            println!("  Chromium manifest not found: {}", path.display());
        }
    }
}

async fn cmd_doctor() {
    println!("🧑‍⚕️ mprisence-web-bridge doctor\n");

    let binary = std::env::current_exe().ok();
    match &binary {
        Some(p) => println!("✓ Binary: {}", p.display()),
        None => println!("✗ Binary: could not resolve"),
    }

    for (browser, dir_fn) in [("Firefox", manifest_dir_firefox as fn() -> std::path::PathBuf),
                               ("Chromium", manifest_dir_chromium as fn() -> std::path::PathBuf)] {
        let dir = dir_fn();
        let manifest_path = dir.join(format!("{HOST_NAME}.json"));
        if manifest_path.exists() {
            match std::fs::read_to_string(&manifest_path) {
                Ok(content) => {
                    if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                        let path_ok = manifest.get("path")
                            .and_then(|p| p.as_str())
                            .map(|p| std::path::Path::new(p).exists())
                            .unwrap_or(false);
                        if path_ok {
                            println!("✓ {browser} manifest: {} (binary OK)", manifest_path.display());
                        } else {
                            println!("⚠ {browser} manifest: {} (binary missing/stale)", manifest_path.display());
                        }
                    } else {
                        println!("⚠ {browser} manifest: {} (invalid JSON)", manifest_path.display());
                    }
                }
                Err(e) => println!("✗ {browser} manifest: {} (read error: {e})", manifest_path.display()),
            }
        } else {
            println!("✗ {browser} manifest: not found at {}", manifest_path.display());
        }
    }

    let dbus_ok = std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok();
    if dbus_ok {
        println!("✓ D-Bus session bus: available");
    } else {
        println!("✗ D-Bus session bus: DBUS_SESSION_BUS_ADDRESS not set");
    }
}

// ─── Debug Fake Player ────────────────────────────────────────────

async fn debug_fake_player(mpris_name: String) {
    info!("Starting fake test player: {mpris_name}");

    let (cmd_tx, mut _cmd_rx) = mpsc::channel::<TaggedCommand>(64);

    let publisher = MprisPublisher::new(&mpris_name, "debug:fake:1", cmd_tx).await.unwrap();
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
            rate: 1.0,
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
            raise: true,
        },
        canonical_url: Some("https://music.youtube.com/watch?v=dQw4w9WgXcQ".into()),
        confidence: ConfidenceLevel::Provider,
        last_seen: std::time::Instant::now(),
    };

    info!("Publishing fake player...");
    publisher.publish(Some(&fake_source)).await;
    info!("Fake player published! Check with: playerctl metadata");
    info!("Running...");
}
