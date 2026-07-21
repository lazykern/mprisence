use std::path::Path;

use mpris::{PlaybackStatus, PlayerFinder};
use serde::{Deserialize, Serialize};
use tiny_http::Method;

use crate::config;
use crate::error::Error;
use crate::metadata::{MediaMetadata, MetadataSource};
use crate::player::{canonical_player_bus_name, is_playerctld_no_active_error};
use crate::template::{RenderContext, TemplateManager};
use crate::utils::format_playback_status_icon;

const INDEX_HTML: &str = include_str!("config_ui.html");
const EXAMPLE_CONFIG: &str = include_str!("../config/config.example.toml");

/// Start the config UI server on a random localhost port and serve forever.
pub fn serve() -> Result<(), Error> {
    let config_path = config::get_config_path()?;
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let addr = server
        .server_addr()
        .to_ip()
        .expect("tcp listener has an ip address");
    let url = format!("http://{}", addr);
    println!("mprisence config ui listening on {url}");
    println!("Editing {}", config_path.display());
    if std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .is_err()
    {
        println!("Open the URL above in your browser (xdg-open not available)");
    }

    // ponytail: single-threaded request loop; one local user, trivial data rate.
    for mut request in server.incoming_requests() {
        let mut body = String::new();
        let _ = request.as_reader().read_to_string(&mut body);
        let (status, content_type, payload) =
            route(request.method(), request.url(), &body, &config_path);
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
            .expect("static content-type header");
        let response = tiny_http::Response::from_string(payload)
            .with_status_code(status)
            .with_header(header);
        let _ = request.respond(response);
    }
    Ok(())
}

fn route(
    method: &Method,
    url: &str,
    body: &str,
    config_path: &Path,
) -> (u16, &'static str, String) {
    match (method, url) {
        (Method::Get, "/") => (200, "text/html; charset=utf-8", INDEX_HTML.to_string()),
        (Method::Get, "/api/config") => get_config_text(config_path),
        (Method::Put, "/api/config") => save_config(config_path, body),
        (Method::Get, "/api/players") => list_players(),
        (Method::Post, "/api/preview") => preview(body),
        _ => (404, "text/plain", "not found".to_string()),
    }
}

fn get_config_text(config_path: &Path) -> (u16, &'static str, String) {
    let text = std::fs::read_to_string(config_path).unwrap_or_else(|_| EXAMPLE_CONFIG.to_string());
    (200, "text/plain; charset=utf-8", text)
}

/// Validate by loading through the production loader on a temp file, then
/// atomically rename over config.toml so the daemon's file watcher never
/// sees a half-written or invalid config.
fn save_config(config_path: &Path, body: &str) -> (u16, &'static str, String) {
    let tmp = config_path.with_extension("toml.uitmp");
    if let Err(e) = std::fs::write(&tmp, body) {
        return (500, "text/plain", e.to_string());
    }
    match config::load_config_from_file(&tmp) {
        Ok(_) => match std::fs::rename(&tmp, config_path) {
            Ok(()) => (204, "text/plain", String::new()),
            Err(e) => (500, "text/plain", e.to_string()),
        },
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            (400, "text/plain", e.to_string())
        }
    }
}

#[derive(Serialize)]
struct PlayerEntry {
    identity: String,
    player_bus_name: String,
    status: Option<String>,
    context: RenderContext,
}

fn list_players() -> (u16, &'static str, String) {
    match collect_players() {
        Ok(entries) => (
            200,
            "application/json",
            serde_json::to_string(&entries).expect("serializable entries"),
        ),
        Err(e) => (500, "text/plain", e.to_string()),
    }
}

fn collect_players() -> Result<Vec<PlayerEntry>, Error> {
    let mut finder = PlayerFinder::new()?;
    finder.set_player_timeout_ms(2000);
    let mut entries = Vec::new();
    for player in finder.iter_players()? {
        let mut player = match player {
            Ok(p) => p,
            Err(e) if is_playerctld_no_active_error(&e) => continue,
            Err(e) => return Err(e.into()),
        };
        player.set_dbus_timeout_ms(2000);
        let status = player
            .get_playback_status()
            .unwrap_or(PlaybackStatus::Stopped);
        let metadata = player
            .get_metadata()
            .map(|m| MetadataSource::from_mpris_with_override(m, None).to_media_metadata())
            .unwrap_or_default();
        let context = RenderContext::new(&player, status, metadata, None);
        entries.push(PlayerEntry {
            identity: player.identity().to_string(),
            player_bus_name: canonical_player_bus_name(player.bus_name()),
            status: context.status.clone(),
            context,
        });
    }
    Ok(entries)
}

#[derive(Deserialize)]
struct PreviewRequest {
    toml: String,
    #[serde(default)]
    player_bus_name: Option<String>,
}

#[derive(Serialize, Default)]
struct PreviewResponse {
    valid: bool,
    error: Option<String>,
    player: Option<String>,
    details: Option<String>,
    state: Option<String>,
    large_text: Option<String>,
    small_text: Option<String>,
}

fn preview(body: &str) -> (u16, &'static str, String) {
    let request: PreviewRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return (400, "text/plain", e.to_string()),
    };
    let response = render_preview(&request);
    (
        200,
        "application/json",
        serde_json::to_string(&response).expect("serializable preview"),
    )
}

fn render_preview(request: &PreviewRequest) -> PreviewResponse {
    let config = match config::parse_config_str(&request.toml) {
        Ok(c) => c,
        Err(e) => {
            return PreviewResponse {
                error: Some(e.to_string()),
                ..Default::default()
            }
        }
    };
    let t = &config.template;
    let manager =
        match TemplateManager::new_raw(&t.details, &t.state, &t.large_text, &t.small_text) {
            Ok(m) => m,
            Err(e) => {
                return PreviewResponse {
                    error: Some(e.to_string()),
                    ..Default::default()
                }
            }
        };
    let (context, player_label) = preview_context(request.player_bus_name.as_deref());
    let render = |name: &str| {
        Some(
            manager
                .render(name, &context)
                .unwrap_or_else(|e| format!("<render error: {e}>")),
        )
    };
    PreviewResponse {
        valid: true,
        error: None,
        player: Some(player_label),
        details: render("details"),
        state: render("state"),
        large_text: render("large_text"),
        small_text: render("small_text"),
    }
}

/// Pick the requested player, else the first Playing one, else the first
/// found, else a hardcoded sample so template editing works with no players.
fn preview_context(bus_name: Option<&str>) -> (RenderContext, String) {
    if let Ok(entries) = collect_players() {
        let chosen = entries
            .iter()
            .position(|e| Some(e.player_bus_name.as_str()) == bus_name)
            .or_else(|| {
                entries
                    .iter()
                    .position(|e| e.status.as_deref() == Some("Playing"))
            })
            .or(if entries.is_empty() { None } else { Some(0) });
        if let Some(index) = chosen {
            let entry = &entries[index];
            return (entry.context.clone(), entry.identity.clone());
        }
    }
    (sample_context(), "Sample".to_string())
}

fn sample_context() -> RenderContext {
    RenderContext {
        player: "Sample Player".to_string(),
        player_bus_name: "sample_player".to_string(),
        status: Some("Playing".to_string()),
        status_icon: Some(format_playback_status_icon(PlaybackStatus::Playing).to_string()),
        volume: Some(0.5),
        metadata: MediaMetadata {
            title: Some("Sample Track".to_string()),
            artists: vec!["Sample Artist".to_string()],
            artist_display: Some("Sample Artist".to_string()),
            album: Some("Sample Album".to_string()),
            duration_secs: Some(215),
            duration_display: Some("03:35".to_string()),
            year: Some("2024".to_string()),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::Method;

    fn tmp_config_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("mprisence-config-ui-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn index_serves_html() {
        let (status, ctype, body) = route(&Method::Get, "/", "", &tmp_config_path("a.toml"));
        assert_eq!(status, 200);
        assert!(ctype.starts_with("text/html"));
        assert!(body.contains("toml"));
    }

    #[test]
    fn unknown_route_is_404() {
        let (status, _, _) = route(&Method::Get, "/nope", "", &tmp_config_path("b.toml"));
        assert_eq!(status, 404);
    }

    #[test]
    fn get_config_falls_back_to_example() {
        let (status, _, body) = route(&Method::Get, "/api/config", "", &tmp_config_path("c.toml"));
        assert_eq!(status, 200);
        assert!(!body.is_empty());
    }

    #[test]
    fn put_invalid_config_is_400_and_not_written() {
        let path = tmp_config_path("d.toml");
        let (status, _, err) = route(&Method::Put, "/api/config", "[template\n", &path);
        assert_eq!(status, 400);
        assert!(!err.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn put_valid_config_is_204_and_written() {
        let path = tmp_config_path("e.toml");
        let (status, _, _) = route(&Method::Put, "/api/config", "clear_on_pause = true\n", &path);
        assert_eq!(status, 204);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("clear_on_pause"));
    }

    #[test]
    fn preview_with_valid_toml_renders() {
        let body = serde_json::json!({ "toml": "", "player_bus_name": null }).to_string();
        let (status, ctype, payload) =
            route(&Method::Post, "/api/preview", &body, &tmp_config_path("f.toml"));
        assert_eq!(status, 200);
        assert!(ctype.starts_with("application/json"));
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["valid"], true);
        assert!(parsed["details"].is_string());
    }

    #[test]
    fn preview_with_invalid_toml_reports_error() {
        let body = serde_json::json!({ "toml": "[template\n" }).to_string();
        let (_, _, payload) =
            route(&Method::Post, "/api/preview", &body, &tmp_config_path("g.toml"));
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["valid"], false);
        assert!(parsed["error"].is_string());
    }

    #[test]
    fn sample_context_renders_with_new_raw() {
        let manager =
            crate::template::TemplateManager::new_raw("{{player}} - {{title}}", "", "", "")
                .unwrap();
        let out = manager.render("details", &sample_context()).unwrap();
        assert_eq!(out, "Sample Player - Sample Track");
    }
}
