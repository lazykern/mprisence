use std::path::Path;

use mpris::{PlaybackStatus, PlayerFinder};
use serde::{Deserialize, Serialize};
use tiny_http::Method;

use crate::config::{self, ConfigManager};
use crate::error::Error;
use crate::metadata::{MediaMetadata, MetadataSource};
use crate::player::{canonical_player_bus_name, is_playerctld_no_active_error};
use crate::template::{RenderContext, TemplateManager};
use crate::utils::{format_playback_status_icon, normalize_player_identity};

const INDEX_HTML: &str = include_str!("config_ui.html");
const EXAMPLE_CONFIG: &str = include_str!("../config/config.example.toml");

/// Start the config UI server on a random localhost port and serve forever.
pub fn serve() -> Result<(), Error> {
    let config_path = config::get_config_path()?;
    let server =
        tiny_http::Server::http("127.0.0.1:0").map_err(|e| std::io::Error::other(e.to_string()))?;
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
        (Method::Get, "/api/settings") => get_settings(config_path),
        (Method::Patch, "/api/settings") => patch_settings(config_path, body),
        (Method::Get, "/api/players") => list_players(config_path),
        (Method::Post, "/api/preview") => preview(config_path, body),
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
    /// Key for `[player.<config_key>]` overrides in config.toml.
    config_key: String,
    allowed: bool,
    status: Option<String>,
    context: RenderContext,
}

/// Effective config loaded from disk (defaults if the file is missing/broken).
fn effective_config(config_path: &Path) -> config::Config {
    config::load_config_from_file(config_path)
        .or_else(|_| config::parse_config_str(""))
        .expect("bundled default config is valid")
}

fn list_players(config_path: &Path) -> (u16, &'static str, String) {
    match collect_players(config_path) {
        Ok(entries) => (
            200,
            "application/json",
            serde_json::to_string(&entries).expect("serializable entries"),
        ),
        Err(e) => (500, "text/plain", e.to_string()),
    }
}

fn collect_players(config_path: &Path) -> Result<Vec<PlayerEntry>, Error> {
    let manager = ConfigManager::new_with_config(effective_config(config_path));
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
        let identity = player.identity().to_string();
        let player_bus_name = canonical_player_bus_name(player.bus_name());
        entries.push(PlayerEntry {
            config_key: normalize_player_identity(&identity),
            allowed: manager.is_player_allowed(&identity, &player_bus_name),
            identity,
            player_bus_name,
            status: context.status.clone(),
            context,
        });
    }
    Ok(entries)
}

/// Template overrides for as-you-type preview; missing fields fall back to
/// the saved config's templates.
#[derive(Deserialize, Default)]
#[serde(default)]
struct PreviewRequest {
    details: Option<String>,
    state: Option<String>,
    large_text: Option<String>,
    small_text: Option<String>,
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

fn preview(config_path: &Path, body: &str) -> (u16, &'static str, String) {
    let request: PreviewRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return (400, "text/plain", e.to_string()),
    };
    let response = render_preview(config_path, &request);
    (
        200,
        "application/json",
        serde_json::to_string(&response).expect("serializable preview"),
    )
}

fn render_preview(config_path: &Path, request: &PreviewRequest) -> PreviewResponse {
    let config = effective_config(config_path);
    let t = &config.template;
    let manager = match TemplateManager::new_raw(
        request.details.as_deref().unwrap_or(&t.details),
        request.state.as_deref().unwrap_or(&t.state),
        request.large_text.as_deref().unwrap_or(&t.large_text),
        request.small_text.as_deref().unwrap_or(&t.small_text),
    ) {
        Ok(m) => m,
        Err(e) => {
            return PreviewResponse {
                error: Some(e.to_string()),
                ..Default::default()
            }
        }
    };
    let (context, player_label) = preview_context(config_path, request.player_bus_name.as_deref());
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
fn preview_context(config_path: &Path, bus_name: Option<&str>) -> (RenderContext, String) {
    if let Ok(entries) = collect_players(config_path) {
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

/// Flat view of the common settings the UI exposes as friendly controls.
#[derive(Serialize)]
struct Settings {
    interval: u64,
    activity_type: String,
    use_content_type: bool,
    time_show: bool,
    time_as_elapsed: bool,
    details: String,
    state: String,
    large_text: String,
    small_text: String,
    cover_providers: Vec<String>,
    imgbb_api_key: Option<String>,
}

fn get_settings(config_path: &Path) -> (u16, &'static str, String) {
    let config = effective_config(config_path);
    let settings = Settings {
        interval: config.interval,
        activity_type: format!("{:?}", config.activity_type.default).to_lowercase(),
        use_content_type: config.activity_type.use_content_type,
        time_show: config.time.show,
        time_as_elapsed: config.time.as_elapsed,
        details: config.template.details.to_string(),
        state: config.template.state.to_string(),
        large_text: config.template.large_text.to_string(),
        small_text: config.template.small_text.to_string(),
        cover_providers: config.cover.provider.provider.clone(),
        imgbb_api_key: config.cover.provider.imgbb.api_key.clone(),
    };
    (
        200,
        "application/json",
        serde_json::to_string(&settings).expect("serializable settings"),
    )
}

/// One key change: `{"path": ["time", "show"], "value": false}`.
/// `value: null` removes the key (reverts to default).
#[derive(Deserialize)]
struct PatchRequest {
    path: Vec<String>,
    value: serde_json::Value,
}

fn patch_settings(config_path: &Path, body: &str) -> (u16, &'static str, String) {
    let request: PatchRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return (400, "text/plain", e.to_string()),
    };
    if request.path.is_empty() {
        return (400, "text/plain", "empty path".to_string());
    }
    let text = std::fs::read_to_string(config_path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => return (400, "text/plain", e.to_string()),
    };
    if let Err(message) = apply_patch(&mut doc, &request) {
        return (400, "text/plain", message);
    }
    // Reuse the validate-on-temp-file + atomic-rename save path.
    save_config(config_path, &doc.to_string())
}

fn apply_patch(doc: &mut toml_edit::DocumentMut, request: &PatchRequest) -> Result<(), String> {
    let (last, parents) = request.path.split_last().expect("checked non-empty");
    let mut table = doc.as_table_mut();
    for key in parents {
        if !table.contains_key(key) {
            let mut implicit = toml_edit::Table::new();
            implicit.set_implicit(true);
            table.insert(key, toml_edit::Item::Table(implicit));
        }
        table = table
            .get_mut(key)
            .and_then(|item| item.as_table_mut())
            .ok_or_else(|| format!("'{key}' is not a table"))?;
    }
    match json_to_toml(&request.value)? {
        Some(value) => {
            table.insert(last, toml_edit::Item::Value(value));
        }
        None => {
            table.remove(last);
        }
    }
    Ok(())
}

fn json_to_toml(value: &serde_json::Value) -> Result<Option<toml_edit::Value>, String> {
    use serde_json::Value as Json;
    Ok(match value {
        Json::Null => None,
        Json::Bool(b) => Some((*b).into()),
        Json::String(s) => Some(s.as_str().into()),
        Json::Number(n) => Some(
            n.as_i64()
                .ok_or_else(|| format!("unsupported number: {n}"))?
                .into(),
        ),
        Json::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items {
                array.push(json_to_toml(item)?.ok_or_else(|| "null in array".to_string())?);
            }
            Some(array.into())
        }
        Json::Object(_) => return Err("objects not supported; patch one key at a time".to_string()),
    })
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
        assert!(body.contains("mprisence"));
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
        let (status, _, _) = route(
            &Method::Put,
            "/api/config",
            "clear_on_pause = true\n",
            &path,
        );
        assert_eq!(status, 204);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("clear_on_pause"));
    }

    #[test]
    fn preview_with_defaults_renders() {
        let body = serde_json::json!({}).to_string();
        let (status, ctype, payload) = route(
            &Method::Post,
            "/api/preview",
            &body,
            &tmp_config_path("f.toml"),
        );
        assert_eq!(status, 200);
        assert!(ctype.starts_with("application/json"));
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["valid"], true);
        assert!(parsed["details"].is_string());
    }

    #[test]
    fn preview_with_broken_template_reports_error() {
        let body = serde_json::json!({ "details": "{{#if}}" }).to_string();
        let (_, _, payload) = route(
            &Method::Post,
            "/api/preview",
            &body,
            &tmp_config_path("g.toml"),
        );
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["valid"], false);
        assert!(parsed["error"].is_string());
    }

    #[test]
    fn get_settings_returns_effective_defaults() {
        let (status, ctype, payload) = route(
            &Method::Get,
            "/api/settings",
            "",
            &tmp_config_path("h.toml"),
        );
        assert_eq!(status, 200);
        assert!(ctype.starts_with("application/json"));
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert!(parsed["interval"].is_u64());
        assert_eq!(parsed["activity_type"], "listening");
        assert!(parsed["details"].is_string());
    }

    #[test]
    fn patch_writes_key_and_preserves_comments() {
        let path = tmp_config_path("i.toml");
        std::fs::write(&path, "# my precious comment\ninterval = 5000\n").unwrap();
        let body = serde_json::json!({ "path": ["time", "show"], "value": false }).to_string();
        let (status, _, _) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 204);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# my precious comment"));
        assert!(saved.contains("interval = 5000"));
        assert!(saved.contains("show = false"));
    }

    #[test]
    fn patch_null_removes_key() {
        let path = tmp_config_path("j.toml");
        std::fs::write(&path, "interval = 5000\n").unwrap();
        let body = serde_json::json!({ "path": ["interval"], "value": null }).to_string();
        let (status, _, _) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 204);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("interval"));
    }

    #[test]
    fn patch_invalid_value_is_400_and_not_written() {
        let path = tmp_config_path("k.toml");
        let body = serde_json::json!({ "path": ["interval"], "value": "soon" }).to_string();
        let (status, _, err) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 400);
        assert!(!err.is_empty());
        assert!(!path.exists());
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
