mod active_source;
mod mpris;
mod native_messaging;
mod protocol;

use active_source::{SourceRegistry, HEARTBEAT_TIMEOUT};
use clap::{Parser, Subcommand};
use log::{debug, error, info, trace, warn};
use mpris::{MprisCommand, MprisPublisher};
use native_messaging::{read_message, send_message};
use protocol::{BridgeMessage, ExtMessage, SourceState};
use std::io::{stdout, Write};
use tokio::io::BufReader;
use tokio::pin;
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
        /// MPRIS bus name suffix.
        #[arg(long, default_value = "web")]
        mpris_name: String,
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
        Some(BridgeCommand::Run { mpris_name }) => run_bridge(mpris_name).await,
        Some(BridgeCommand::Install { browser }) => cmd_install(browser).await,
        Some(BridgeCommand::Uninstall { browser }) => cmd_uninstall(browser).await,
        Some(BridgeCommand::Doctor) => cmd_doctor().await,
        Some(BridgeCommand::DebugFakePlayer { mpris_name }) => debug_fake_player(mpris_name).await,
        None => {
            run_bridge("web".into()).await;
        }
    }
}

// ─── Run ──────────────────────────────────────────────────────────

async fn run_bridge(mpris_name: String) {
    info!("Starting mprisence-web-bridge (mpris_name: {mpris_name})");

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<MprisCommand>(64);

    let publisher = match MprisPublisher::new(&mpris_name, cmd_tx).await {
        Ok(p) => {
            info!("MPRIS player published: {}", p.bus_name());
            p
        }
        Err(e) => {
            error!("Failed to publish MPRIS player: {e}");
            return;
        }
    };

    let mpris_run = publisher.run_task();
    pin!(mpris_run);

    let mut registry = SourceRegistry::new();

    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin);

    let mut heartbeat_timer = interval(Duration::from_secs(2));
    let mut mpris_done = false;

    loop {
        tokio::select! {
            _ = &mut mpris_run => {
                info!("MPRIS server stopped");
                mpris_done = true;
                break;
            }

            msg_result = read_message(&mut stdin_reader) => {
                match msg_result {
                    Ok(Some(bytes)) => {
                        match serde_json::from_slice::<ExtMessage>(&bytes) {
                            Ok(ext_msg) => {
                                trace!("← ext: {}", String::from_utf8_lossy(
                                    &bytes[..bytes.len().min(200)]));
                                handle_extension_message(ext_msg, &mut registry,
                                    &publisher, &mut stdout()).await;
                            }
                            Err(e) => warn!("Failed to parse: {e}"),
                        }
                    }
                    Ok(None) => { info!("Browser disconnected (EOF)"); break; }
                    Err(e) => { error!("stdin error: {e}"); break; }
                }
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(cmd) => {
                        trace!("← MPRIS cmd: {cmd:?}");
                        handle_mpris_command(cmd, &registry, &mut stdout()).await;
                    }
                    None => { info!("MPRIS channel closed"); break; }
                }
            }

            _ = heartbeat_timer.tick() => {
                registry.prune_stale();
                let (active, _reason) = registry.select_active();
                if let Some(source) = active {
                    if source.is_stale(HEARTBEAT_TIMEOUT) {
                        publisher.publish(None).await;
                    } else {
                        publisher.publish(Some(source)).await;
                    }
                } else {
                    publisher.publish(None).await;
                }
            }
        }
    }

    if !mpris_done {
        publisher.publish(None).await;
    }
    info!("Bridge shutting down");
}

async fn handle_extension_message(
    msg: ExtMessage,
    registry: &mut SourceRegistry,
    publisher: &MprisPublisher,
    stdout: &mut impl Write,
) {
    match msg {
        ExtMessage::Hello { browser, extension_version, protocol, git_sha } => {
            info!("Extension connected: {browser:?} v{extension_version}");

            // Protocol version check
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

        ExtMessage::Update { source_id, url, origin, site, playback, metadata, capabilities, confidence } => {
            let state = SourceState {
                source_id: source_id.clone(),
                url, origin, site,
                playback, metadata, capabilities, confidence,
                last_seen: std::time::Instant::now(),
            };
            registry.upsert(state);
            if registry.source_count() == 1 {
                let (active, _) = registry.select_active();
                if let Some(s) = active {
                    publisher.publish(Some(s)).await;
                }
            }
        }

        ExtMessage::Remove { source_id } => {
            debug!("Source removed: {source_id}");
            registry.remove(&source_id);
        }
    }
}

async fn handle_mpris_command(
    cmd: MprisCommand,
    registry: &SourceRegistry,
    stdout: &mut impl Write,
) {
    let active_id = match registry.active_source_id() {
        Some(id) => id.to_string(),
        None => { trace!("No active source for {cmd:?}"); return; }
    };

    let bridge_cmd = match cmd {
        MprisCommand::PlayPause => protocol::CommandKind::PlayPause,
        MprisCommand::Next => protocol::CommandKind::Next,
        MprisCommand::Previous => protocol::CommandKind::Previous,

        MprisCommand::Seek(offset_us) => {
            let msg = BridgeMessage::Command {
                source_id: active_id,
                command: protocol::CommandKind::Seek,
                position_ms: Some((offset_us / 1000) as u64),
            };
            let _ = send_message(stdout, &msg);
            return;
        }
        MprisCommand::SetPosition(pos_us) => {
            let msg = BridgeMessage::Command {
                source_id: active_id,
                command: protocol::CommandKind::SetPosition,
                position_ms: Some((pos_us / 1000) as u64),
            };
            let _ = send_message(stdout, &msg);
            return;
        }
        MprisCommand::Play | MprisCommand::Pause | MprisCommand::Stop => {
            protocol::CommandKind::PlayPause
        }
    };

    let msg = BridgeMessage::Command {
        source_id: active_id,
        command: bridge_cmd,
        position_ms: None,
    };
    if let Err(e) = send_message(stdout, &msg) {
        warn!("Failed to send command: {e}");
    }
}

// ─── Install / Uninstall / Doctor ────────────────────────────────

/// Detect which browsers are available and write native messaging host manifests.
/// The manifest tells the browser where the bridge binary is and which extension
/// is allowed to connect.
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

    // Chromium also supports "allowed_origins" instead of "allowed_extensions".
    // For local development, we use allowed_extensions with the extension ID.
    // When published to the Chrome Web Store, switch to allowed_origins.

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

    // 1. Binary path
    let binary = std::env::current_exe().ok();
    match &binary {
        Some(p) => println!("✓ Binary: {}", p.display()),
        None => println!("✗ Binary: could not resolve"),
    }

    // 2. Native messaging manifests
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

    // 3. D-Bus session bus
    let dbus_ok = std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok();
    if dbus_ok {
        println!("✓ D-Bus session bus: available");
    } else {
        println!("✗ D-Bus session bus: DBUS_SESSION_BUS_ADDRESS not set");
    }

    // 4. MPRIS
    println!("  MPRIS: test with `playerctl -l | grep mprisence` after connecting");
}

// ─── Debug Fake Player ────────────────────────────────────────────

async fn debug_fake_player(mpris_name: String) {
    info!("Starting fake test player: {mpris_name}");

    let (cmd_tx, mut _cmd_rx) = mpsc::channel::<MprisCommand>(64);
    let publisher = match MprisPublisher::new(&mpris_name, cmd_tx).await {
        Ok(p) => p,
        Err(e) => { error!("Failed: {e}"); return; }
    };

    let mpris_run = publisher.run_task();
    pin!(mpris_run);

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
        confidence: ConfidenceLevel::Provider,
        last_seen: std::time::Instant::now(),
    };

    info!("Publishing fake player...");
    publisher.publish(Some(&fake_source)).await;
    info!("Fake player published! Check with: playerctl metadata");
    info!("Running...");

    let mut timer = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = &mut mpris_run => break,
            _ = timer.tick() => {}
        }
    }
}
