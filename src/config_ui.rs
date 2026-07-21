use std::path::Path;

use mpris::{PlaybackStatus, PlayerFinder};
use serde::{Deserialize, Serialize};
use tiny_http::Method;

use crate::config::schema::{PlayerConfig, PlayerConfigLayer, WebPlayerConfigLayer};
use crate::config::{self};
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
        // Binary route: cover art proxy (route() only speaks strings).
        if request.method() == &Method::Get && request.url().starts_with("/api/art?") {
            let url = request.url().to_string();
            let _ = match art_bytes(&config_path, &url) {
                Some(bytes) => request.respond(tiny_http::Response::from_data(bytes)),
                None => request
                    .respond(tiny_http::Response::from_data(Vec::new()).with_status_code(404)),
            };
            continue;
        }
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
        (Method::Get, "/api/web_players") => list_web_players(config_path),
        (Method::Post, "/api/preview") => preview(config_path, body),
        _ => (404, "text/plain", "not found".to_string()),
    }
}

/// Serve the cover art file a player currently reports as a `file://` URL.
/// Only paths advertised by a live player are readable — never arbitrary
/// request-supplied paths.
fn art_bytes(config_path: &Path, url: &str) -> Option<Vec<u8>> {
    let parsed = url::Url::parse(&format!("http://localhost{url}")).ok()?;
    let bus = parsed
        .query_pairs()
        .find(|(k, _)| k == "player_bus_name")?
        .1
        .into_owned();
    let entries = collect_players(config_path).ok()?;
    let art = entries
        .iter()
        .find(|e| e.player_bus_name == bus)?
        .art_url
        .clone()?;
    let path = url::Url::parse(&art).ok()?.to_file_path().ok()?;
    std::fs::read(path).ok()
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
    art_url: Option<String>,
    context: RenderContext,
    /// Effective per-player config the daemon uses (defaults + overrides).
    resolved: PlayerConfig,
    /// The user's explicit `[player.<config_key>]` layer, so the UI knows
    /// which fields are overridden (and can show a per-field reset).
    overrides: PlayerConfigLayer,
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
    let config = effective_config(config_path);
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
        let mpris_metadata = player.get_metadata().ok();
        let art_url = mpris_metadata
            .as_ref()
            .and_then(|m| m.art_url())
            .map(String::from);
        let metadata = mpris_metadata
            .map(|m| MetadataSource::from_mpris_with_override(m, None).to_media_metadata())
            .unwrap_or_default();
        let context = RenderContext::new(&player, status, metadata, None);
        let identity = player.identity().to_string();
        let player_bus_name = canonical_player_bus_name(player.bus_name());
        let config_key = normalize_player_identity(&identity);
        entries.push(PlayerEntry {
            allowed: config.is_player_allowed(&identity, &player_bus_name),
            resolved: config.get_player_config(&identity, &player_bus_name),
            overrides: config.user_player.get(&config_key).cloned().unwrap_or_default(),
            config_key,
            identity,
            player_bus_name,
            status: context.status.clone(),
            art_url,
            context,
        });
    }
    Ok(entries)
}

/// One configured web-player site (bundled and/or user-overridden).
#[derive(Serialize)]
struct WebPlayerEntry {
    /// Key for `[web_player.<key>]` in config.toml.
    key: String,
    /// True if this key ships in the bundled defaults.
    bundled: bool,
    /// Merged (effective) layer the daemon matches against.
    effective: WebPlayerConfigLayer,
    /// The user's explicit `[web_player.<key>]` layer, for per-field reset.
    overrides: WebPlayerConfigLayer,
}

/// List every configured web player (bundled defaults merged with user
/// overrides). Sorted with `default` first, then alphabetically.
fn list_web_players(config_path: &Path) -> (u16, &'static str, String) {
    let config = effective_config(config_path);
    let mut keys: Vec<&String> = config.merged_web_player.keys().collect();
    keys.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("default", "default") => std::cmp::Ordering::Equal,
        ("default", _) => std::cmp::Ordering::Less,
        (_, "default") => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });
    let entries: Vec<WebPlayerEntry> = keys
        .into_iter()
        .map(|key| WebPlayerEntry {
            bundled: config.bundled_web_player.contains_key(key),
            effective: config.merged_web_player.get(key).cloned().unwrap_or_default(),
            overrides: config.user_web_player.get(key).cloned().unwrap_or_default(),
            key: key.clone(),
        })
        .collect();
    (
        200,
        "application/json",
        serde_json::to_string(&entries).expect("serializable web players"),
    )
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
    error: Option<String>,
    player: Option<String>,
    /// Playback status of the previewed player ("Playing"/"Paused"/"Stopped"),
    /// so the UI can flag when a preview isn't actually live in Discord.
    status: Option<String>,
    art_url: Option<String>,
    details: Option<String>,
    state: Option<String>,
    large_text: Option<String>,
    small_text: Option<String>,
    status_icon: Option<String>,
    duration: Option<String>,
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
    let (context, player_label, art_url) =
        preview_context(config_path, request.player_bus_name.as_deref());
    let render = |name: &str| {
        Some(
            manager
                .render(name, &context)
                .unwrap_or_else(|e| format!("<render error: {e}>")),
        )
    };
    PreviewResponse {
        error: None,
        player: Some(player_label),
        status: context.status.clone(),
        art_url,
        details: render("details"),
        state: render("state"),
        large_text: render("large_text"),
        small_text: render("small_text"),
        status_icon: context.status_icon.clone(),
        duration: context.metadata.duration_display.clone(),
    }
}

/// Pick the requested player, else the first Playing one, else the first
/// found. Falls back to a hardcoded sample when nothing is picked or the
/// picked player has no current track, so template editing always previews.
fn preview_context(
    config_path: &Path,
    bus_name: Option<&str>,
) -> (RenderContext, String, Option<String>) {
    if let Ok(entries) = collect_players(config_path) {
        if let Some(entry) = pick_preview_entry(&entries, bus_name) {
            // file:// art can't load in a browser page; point it at our proxy.
            let art_url = entry.art_url.as_deref().map(|u| {
                if u.starts_with("file://") {
                    format!("/api/art?player_bus_name={}", entry.player_bus_name)
                } else {
                    u.to_string()
                }
            });
            return (entry.context.clone(), entry.identity.clone(), art_url);
        }
    }
    (sample_context(), "sample track".to_string(), None)
}

fn pick_preview_entry<'a>(
    entries: &'a [PlayerEntry],
    bus_name: Option<&str>,
) -> Option<&'a PlayerEntry> {
    entries
        .iter()
        .find(|e| Some(e.player_bus_name.as_str()) == bus_name)
        .or_else(|| {
            entries
                .iter()
                .find(|e| e.status.as_deref() == Some("Playing"))
        })
        .or_else(|| entries.first())
        .filter(|e| e.context.metadata.title.is_some())
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
    event_driven: bool,
    fallback_poll_interval: u64,
    allowed_players: Vec<String>,
    activity_type: String,
    use_content_type: bool,
    time_show: bool,
    time_as_elapsed: bool,
    details: String,
    state: String,
    large_text: String,
    small_text: String,
    cover_providers: Vec<String>,
    cover_file_names: Vec<String>,
    cover_local_search_depth: usize,
    musicbrainz_min_score: u8,
    imgbb_api_key: Option<String>,
    imgbb_expiration: u64,
    catbox_user_hash: Option<String>,
    catbox_use_litter: bool,
    catbox_litter_hours: u8,
    /// Default template strings, so the UI can show a per-field reset.
    defaults: TemplateDefaults,
}

#[derive(Serialize)]
struct TemplateDefaults {
    details: String,
    state: String,
    large_text: String,
    small_text: String,
}

fn get_settings(config_path: &Path) -> (u16, &'static str, String) {
    let config = effective_config(config_path);
    let default = config::parse_config_str("").expect("bundled default config is valid");
    let settings = Settings {
        interval: config.interval,
        event_driven: config.event_driven,
        fallback_poll_interval: config.fallback_poll_interval,
        allowed_players: config.allowed_players.clone(),
        activity_type: format!("{:?}", config.activity_type.default).to_lowercase(),
        use_content_type: config.activity_type.use_content_type,
        time_show: config.time.show,
        time_as_elapsed: config.time.as_elapsed,
        details: config.template.details.to_string(),
        state: config.template.state.to_string(),
        large_text: config.template.large_text.to_string(),
        small_text: config.template.small_text.to_string(),
        cover_providers: config.cover.provider.provider.clone(),
        cover_file_names: config.cover.file_names.clone(),
        cover_local_search_depth: config.cover.local_search_depth,
        musicbrainz_min_score: config.cover.provider.musicbrainz.min_score,
        imgbb_api_key: config.cover.provider.imgbb.api_key.clone(),
        imgbb_expiration: config.cover.provider.imgbb.expiration,
        catbox_user_hash: config.cover.provider.catbox.user_hash.clone(),
        catbox_use_litter: config.cover.provider.catbox.use_litter,
        catbox_litter_hours: config.cover.provider.catbox.litter_hours,
        defaults: TemplateDefaults {
            details: default.template.details.to_string(),
            state: default.template.state.to_string(),
            large_text: default.template.large_text.to_string(),
            small_text: default.template.small_text.to_string(),
        },
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
        assert!(parsed["error"].is_null());
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
        assert!(parsed["error"].is_string());
    }

    #[test]
    fn get_settings_exposes_extended_fields() {
        let (status, _, payload) =
            route(&Method::Get, "/api/settings", "", &tmp_config_path("set2.toml"));
        assert_eq!(status, 200);
        let p: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert!(p["event_driven"].is_boolean());
        assert!(p["fallback_poll_interval"].is_u64());
        assert!(p["allowed_players"].is_array());
        assert!(p["cover_file_names"].is_array());
        assert!(p["musicbrainz_min_score"].is_u64());
        assert!(p["catbox_use_litter"].is_boolean());
        assert!(p["catbox_litter_hours"].is_u64());
        assert!(p["imgbb_expiration"].is_u64());
    }

    #[test]
    fn web_players_lists_bundled_sites() {
        let (status, ctype, payload) =
            route(&Method::Get, "/api/web_players", "", &tmp_config_path("web.toml"));
        assert_eq!(status, 200);
        assert!(ctype.starts_with("application/json"));
        let list: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let arr = list.as_array().unwrap();
        assert!(!arr.is_empty(), "bundled web players should be listed");
        assert!(arr.iter().any(|e| e["key"] == "youtube"));
        assert!(arr.iter().any(|e| e["bundled"] == true));
    }

    #[test]
    fn patch_web_player_writes_nested_key() {
        let path = tmp_config_path("webpatch.toml");
        let body =
            serde_json::json!({ "path": ["web_player", "youtube", "ignore"], "value": false })
                .to_string();
        let (status, _, _) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 204);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("[web_player.youtube]"));
        assert!(saved.contains("ignore = false"));
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

    fn fake_entry(bus: &str, status: &str, title: Option<&str>) -> PlayerEntry {
        let mut context = sample_context();
        context.status = Some(status.to_string());
        context.metadata.title = title.map(String::from);
        PlayerEntry {
            identity: bus.to_string(),
            player_bus_name: bus.to_string(),
            config_key: bus.to_string(),
            allowed: true,
            status: Some(status.to_string()),
            art_url: None,
            context,
            resolved: PlayerConfig::default(),
            overrides: PlayerConfigLayer::default(),
        }
    }

    #[test]
    fn pick_preview_entry_prefers_playing_and_skips_trackless() {
        let entries = vec![
            fake_entry("stopped_no_track", "Stopped", None),
            fake_entry("playing", "Playing", Some("Song")),
        ];
        let picked = pick_preview_entry(&entries, None).unwrap();
        assert_eq!(picked.player_bus_name, "playing");

        // Requested player wins over Playing.
        let picked = pick_preview_entry(&entries, Some("stopped_no_track"));
        assert!(picked.is_none(), "trackless pick falls back to sample");

        // No track anywhere: sample fallback.
        let entries = vec![fake_entry("stopped_no_track", "Stopped", None)];
        assert!(pick_preview_entry(&entries, None).is_none());
    }
}
