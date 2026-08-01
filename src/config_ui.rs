use std::path::Path;

use mpris::{PlaybackStatus, PlayerFinder};
use serde::{Deserialize, Serialize};
use tiny_http::Method;

use crate::config::schema::{
    PlayerConfig, PlayerConfigLayer, PlayerConfigMatch, WebPlayerConfigLayer,
};
use crate::config::{self};
use crate::error::Error;
use crate::metadata::{MediaMetadata, MetadataSource};
use crate::player::{
    canonical_player_bus_name, is_mprisence_web_bridge_bus, is_playerctld_no_active_error,
    BRIDGE_CONFIG_KEY,
};
use crate::presence::{determine_activity_type, resolve_status_display_type};
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
        (Method::Get, "/favicon.ico") => (204, "image/x-icon", String::new()),
        (Method::Get, "/api/config") => get_config_text(config_path),
        (Method::Put, "/api/config") => save_config(config_path, body),
        (Method::Post, "/api/config/validate") => validate_config(body),
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
struct ConfigValidation {
    valid: bool,
    error: Option<String>,
    warnings: Vec<String>,
}

fn validate_config(body: &str) -> (u16, &'static str, String) {
    let warnings = config_warnings(body);
    let validation = match config::parse_config_str(body) {
        Ok(_) => ConfigValidation {
            valid: true,
            error: None,
            warnings,
        },
        Err(error) => ConfigValidation {
            valid: false,
            error: Some(error.to_string()),
            warnings,
        },
    };
    (
        200,
        "application/json",
        serde_json::to_string(&validation).expect("serializable validation"),
    )
}

fn config_warnings(body: &str) -> Vec<String> {
    let Ok(toml::Value::Table(root)) = toml::from_str::<toml::Value>(body) else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    warn_unknown_keys(
        "",
        &root,
        &[
            "interval",
            "event_driven",
            "fallback_poll_interval",
            "discovery_interval",
            "allowed_players",
            "web_player_enabled",
            "template",
            "time",
            "cover",
            "activity_type",
            "player",
            "web_player",
        ],
        &mut warnings,
    );
    warn_table(
        &root,
        "template",
        &["details", "detail", "state", "large_text", "small_text"],
        &mut warnings,
    );
    warn_table(&root, "time", &["show", "as_elapsed"], &mut warnings);
    warn_table(
        &root,
        "activity_type",
        &["use_content_type", "default"],
        &mut warnings,
    );
    warn_table(
        &root,
        "cover",
        &["file_names", "provider", "local_search_depth"],
        &mut warnings,
    );
    if let Some(provider) = table_at(&root, &["cover", "provider"]) {
        warn_unknown_keys(
            "cover.provider",
            provider,
            &["provider", "imgbb", "musicbrainz", "catbox"],
            &mut warnings,
        );
        warn_nested_table(
            provider,
            "cover.provider",
            "imgbb",
            &["api_key", "expiration"],
            &mut warnings,
        );
        warn_nested_table(
            provider,
            "cover.provider",
            "musicbrainz",
            &["min_score"],
            &mut warnings,
        );
        warn_nested_table(
            provider,
            "cover.provider",
            "catbox",
            &["user_hash", "use_litter", "litter_hours"],
            &mut warnings,
        );
    }
    // `ignore_unmatched` is a [player.default] concept only — web players hide
    // unmatched sites unconditionally.
    // `allow_streaming` is a [player.*] concept only — a web player matches
    // because its URL is http(s), so disabling streaming there is just
    // `ignore = true`.
    for (section, extra) in [
        ("player", ["ignore_unmatched", "allow_streaming"].as_slice()),
        ("web_player", ["title_suffix"].as_slice()),
    ] {
        if let Some(entries) = root.get(section).and_then(toml::Value::as_table) {
            for (key, value) in entries {
                if let Some(layer) = value.as_table() {
                    let mut allowed = vec![
                        "match_pattern",
                        "match_patterns",
                        "name",
                        "ignore",
                        "app_id",
                        "icon",
                        "show_icon",
                        "status_display_type",
                        "override_activity_type",
                    ];
                    allowed.extend_from_slice(extra);
                    warn_unknown_keys(&format!("{section}.{key}"), layer, &allowed, &mut warnings);
                    if section == "web_player" && key == "default" && layer.contains_key("ignore") {
                        warnings.push(
                            "web_player.default.ignore is no longer used; set web_player_enabled = false to turn web players off"
                                .to_string(),
                        );
                    }
                }
            }
        }
    }
    if root.contains_key("discovery_interval") {
        warnings.push("discovery_interval is deprecated; use fallback_poll_interval".to_string());
    }
    if let Some(template) = root.get("template").and_then(toml::Value::as_table) {
        if template.contains_key("detail") {
            warnings.push("template.detail is deprecated; use template.details".to_string());
        }
    }
    if root.contains_key("clear_on_pause") {
        warnings.push("clear_on_pause was removed and has no effect".to_string());
    }
    warnings
}

fn table_at<'a>(root: &'a toml::value::Table, path: &[&str]) -> Option<&'a toml::value::Table> {
    let mut value = root.get(*path.first()?)?;
    for key in &path[1..] {
        value = value.get(*key)?;
    }
    value.as_table()
}

fn warn_table(root: &toml::value::Table, key: &str, allowed: &[&str], warnings: &mut Vec<String>) {
    if let Some(table) = root.get(key).and_then(toml::Value::as_table) {
        warn_unknown_keys(key, table, allowed, warnings);
    }
}

fn warn_nested_table(
    root: &toml::value::Table,
    prefix: &str,
    key: &str,
    allowed: &[&str],
    warnings: &mut Vec<String>,
) {
    if let Some(table) = root.get(key).and_then(toml::Value::as_table) {
        warn_unknown_keys(&format!("{prefix}.{key}"), table, allowed, warnings);
    }
}

fn warn_unknown_keys(
    prefix: &str,
    table: &toml::value::Table,
    allowed: &[&str],
    warnings: &mut Vec<String>,
) {
    for key in table.keys().filter(|key| !allowed.contains(&key.as_str())) {
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        warnings.push(format!("Unknown setting: {path}"));
    }
}

#[derive(Clone, Serialize)]
struct PlayerEntry {
    identity: String,
    player_bus_name: String,
    /// Key for `[player.<config_key>]` overrides in config.toml.
    config_key: String,
    /// False for the synthetic `default` layer and configured-but-not-running
    /// entries, so the UI can render them as config-only rows.
    live: bool,
    bundled: bool,
    allowed: bool,
    status: Option<String>,
    art_url: Option<String>,
    context: RenderContext,
    /// Effective per-player config the daemon uses (defaults + overrides).
    resolved: PlayerConfig,
    effective: PlayerConfigLayer,
    /// The user's explicit `[player.<config_key>]` layer, so the UI knows
    /// which fields are overridden (and can show a per-field reset).
    overrides: PlayerConfigLayer,
    matches: Vec<PlayerConfigMatch>,
    web_player_key: Option<String>,
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
    let config = config::load_config_from_file(config_path)?;
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
        let url = mpris_metadata
            .as_ref()
            .and_then(|m| m.url().map(String::from));
        let title = mpris_metadata
            .as_ref()
            .and_then(|m| m.title().map(String::from));
        let art_url = mpris_metadata
            .as_ref()
            .and_then(|m| m.art_url())
            .map(String::from);
        let metadata = mpris_metadata
            .map(|m| MetadataSource::from_mpris_with_override(m, None).to_media_metadata())
            .unwrap_or_default();
        let mut context = RenderContext::new(&player, status, metadata, None);
        let identity = player.identity().to_string();
        let player_bus_name = canonical_player_bus_name(player.bus_name());
        let (config_identity, config_bus) = if is_mprisence_web_bridge_bus(&player_bus_name) {
            (BRIDGE_CONFIG_KEY, BRIDGE_CONFIG_KEY)
        } else {
            (identity.as_str(), player_bus_name.as_str())
        };
        let resolution = config.resolve_source(
            config_identity,
            config_bus,
            url.as_deref(),
            title.as_deref(),
        );
        if let Some(name) = resolution.config.name.as_ref() {
            context.player.clone_from(name);
        }
        let player_keys: Vec<String> = resolution
            .player_matches
            .iter()
            .map(|matched| matched.config_key.clone())
            .collect();
        let allowed = config.is_source_allowed(
            &identity,
            &player_bus_name,
            &player_keys,
            resolution.web_player_key.as_deref(),
        );
        let config_key = resolution
            .player_matches
            .last()
            .map(|matched| matched.config_key.clone())
            .unwrap_or_else(|| normalize_player_identity(&identity));
        entries.push(PlayerEntry {
            allowed,
            resolved: resolution.config,
            effective: config
                .merged_player
                .get(&config_key)
                .cloned()
                .unwrap_or_default(),
            overrides: config
                .user_player
                .get(&config_key)
                .cloned()
                .unwrap_or_default(),
            matches: resolution.player_matches,
            web_player_key: resolution.web_player_key,
            live: true,
            bundled: config.bundled_player.contains_key(&config_key),
            config_key,
            identity,
            player_bus_name,
            status: context.status.clone(),
            art_url,
            context,
        });
    }

    append_configured_only(&mut entries, &config);
    Ok(entries)
}

/// Append config-only player rows: the `[player.default]` layer and every
/// bundled or user-defined `[player.*]` key that isn't currently running.
/// `default` is first, followed by the complete preset catalog.
fn append_configured_only(entries: &mut Vec<PlayerEntry>, config: &config::Config) {
    let effective_configs = config.effective_player_configs();
    let mut offline: Vec<String> = config
        .merged_player
        .keys()
        .filter(|k| k.as_str() != "default" && !entries.iter().any(|e| &e.config_key == *k))
        .cloned()
        .collect();
    offline.sort();
    for key in std::iter::once("default".to_string()).chain(offline) {
        let is_default = key == "default";
        let player_keys = [key.clone()];
        let identity_allowed = config.is_player_allowed(&key, &key);
        entries.push(PlayerEntry {
            allowed: is_default
                || identity_allowed
                || config.is_source_allowed(&key, &key, &player_keys, None),
            resolved: effective_configs
                .get(&key)
                .cloned()
                .unwrap_or_else(|| config.get_player_config(&key, &key)),
            effective: config.merged_player.get(&key).cloned().unwrap_or_default(),
            overrides: config.user_player.get(&key).cloned().unwrap_or_default(),
            matches: Vec::new(),
            web_player_key: None,
            live: false,
            bundled: config.bundled_player.contains_key(&key),
            identity: if is_default {
                "Default (all players)".to_string()
            } else {
                key.clone()
            },
            player_bus_name: String::new(),
            status: None,
            art_url: None,
            context: empty_context(),
            config_key: key,
        });
    }
}

/// A blank render context for config-only player rows (no live track).
fn empty_context() -> RenderContext {
    RenderContext {
        player: String::new(),
        player_bus_name: String::new(),
        status: None,
        status_icon: None,
        volume: None,
        metadata: MediaMetadata::default(),
    }
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
    let config = match config::load_config_from_file(config_path) {
        Ok(config) => config,
        Err(error) => return (400, "text/plain", error.to_string()),
    };
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
            effective: config.effective_web_player_layer(key),
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
    source_scope: Option<String>,
    source_key: Option<String>,
}

#[derive(Serialize, Default)]
struct PreviewResponse {
    error: Option<String>,
    player: Option<String>,
    /// Playback status of the previewed player ("Playing"/"Paused"/"Stopped"),
    /// so the UI can flag when a preview isn't actually live in Discord.
    status: Option<String>,
    art_url: Option<String>,
    large_image_url: Option<String>,
    small_image_url: Option<String>,
    icon_url: Option<String>,
    details: Option<String>,
    state: Option<String>,
    large_text: Option<String>,
    small_text: Option<String>,
    status_icon: Option<String>,
    duration: Option<String>,
    context: Option<RenderContext>,
    activity_type: Option<String>,
    status_display_type: Option<String>,
    show_icon: bool,
    allow_streaming: bool,
    live: bool,
    allowed: bool,
    ignored: bool,
    config_key: Option<String>,
    web_player_key: Option<String>,
    matches: Vec<PlayerConfigMatch>,
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
    let config = match config::load_config_from_file(config_path) {
        Ok(config) => config,
        Err(error) => {
            return PreviewResponse {
                error: Some(error.to_string()),
                ..Default::default()
            }
        }
    };
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
    let selection = preview_context(
        config_path,
        &config,
        request.player_bus_name.as_deref(),
        request.source_scope.as_deref(),
        request.source_key.as_deref(),
    );
    let context = &selection.context;
    let render = |name: &str| {
        Some(
            manager
                .render(name, &context)
                .unwrap_or_else(|e| format!("<render error: {e}>")),
        )
    };
    let activity_type = determine_activity_type(
        &config.activity_type,
        &selection.player_config,
        context.metadata.url.as_deref(),
    );
    let status_display_type = resolve_status_display_type(&selection.player_config);
    let icon_url =
        (!selection.player_config.icon.is_empty()).then(|| selection.player_config.icon.clone());
    let (large_image_url, small_image_url) = if selection.art_url.is_some() {
        (
            selection.art_url.clone(),
            selection
                .player_config
                .show_icon
                .then(|| selection.player_config.icon.clone()),
        )
    } else {
        (icon_url.clone(), None)
    };
    PreviewResponse {
        error: None,
        player: Some(selection.player_label),
        status: context.status.clone(),
        art_url: selection.art_url,
        large_image_url,
        small_image_url,
        icon_url,
        details: render("details"),
        state: render("state"),
        large_text: render("large_text"),
        small_text: render("small_text"),
        status_icon: context.status_icon.clone(),
        duration: context.metadata.duration_display.clone(),
        context: Some(context.clone()),
        activity_type: Some(format!("{activity_type:?}").to_lowercase()),
        status_display_type: Some(format!("{status_display_type:?}").to_lowercase()),
        show_icon: selection.player_config.show_icon,
        allow_streaming: selection.player_config.allow_streaming,
        live: selection.live,
        allowed: selection.allowed,
        ignored: selection.player_config.ignore,
        config_key: selection.config_key,
        web_player_key: selection.web_player_key,
        matches: selection.matches,
    }
}

struct PreviewSelection {
    context: RenderContext,
    player_label: String,
    art_url: Option<String>,
    player_config: PlayerConfig,
    live: bool,
    allowed: bool,
    config_key: Option<String>,
    web_player_key: Option<String>,
    matches: Vec<PlayerConfigMatch>,
}

/// Pick the requested player, else the first Playing one, else the first
/// found. Falls back to a hardcoded sample when nothing is picked or the
/// picked player has no current track, so template editing always previews.
fn preview_context(
    config_path: &Path,
    config: &crate::config::schema::Config,
    bus_name: Option<&str>,
    source_scope: Option<&str>,
    source_key: Option<&str>,
) -> PreviewSelection {
    let entries = collect_players(config_path).unwrap_or_default();
    if bus_name.is_some() {
        if let Some(entry) = pick_preview_entry(&entries, bus_name) {
            return preview_selection_from_entry(entry);
        }
    }
    if let (Some(scope), Some(key)) = (source_scope, source_key) {
        if let Some(selection) = scoped_sample_preview(config, &entries, scope, key) {
            return selection;
        }
    }
    if let Some(entry) = pick_preview_entry(&entries, None) {
        return preview_selection_from_entry(entry);
    }
    PreviewSelection {
        context: sample_context(),
        player_label: "Sample Player".to_string(),
        art_url: None,
        player_config: PlayerConfig::default(),
        live: false,
        allowed: true,
        config_key: None,
        web_player_key: None,
        matches: Vec::new(),
    }
}

fn preview_selection_from_entry(entry: &PlayerEntry) -> PreviewSelection {
    let art_url = entry.art_url.as_deref().map(|url| {
        if url.starts_with("file://") {
            format!("/api/art?player_bus_name={}", entry.player_bus_name)
        } else {
            url.to_string()
        }
    });
    PreviewSelection {
        context: entry.context.clone(),
        player_label: entry.identity.clone(),
        art_url,
        player_config: entry.resolved.clone(),
        live: entry.live,
        allowed: entry.allowed,
        config_key: Some(entry.config_key.clone()),
        web_player_key: entry.web_player_key.clone(),
        matches: entry.matches.clone(),
    }
}

fn scoped_sample_preview(
    config: &crate::config::schema::Config,
    entries: &[PlayerEntry],
    scope: &str,
    key: &str,
) -> Option<PreviewSelection> {
    if scope == "player" {
        let entry = entries
            .iter()
            .find(|entry| entry.config_key == key && entry.web_player_key.is_none())?;
        let player_label = entry
            .resolved
            .name
            .clone()
            .unwrap_or_else(|| entry.identity.clone());
        let mut context = sample_context();
        context.player = player_label.clone();
        context.player_bus_name = key.to_string();
        return Some(PreviewSelection {
            context,
            player_label,
            art_url: None,
            player_config: entry.resolved.clone(),
            live: false,
            allowed: entry.allowed,
            config_key: Some(key.to_string()),
            web_player_key: None,
            matches: entry.matches.clone(),
        });
    }
    if scope != "web_player" {
        return None;
    }

    let web = config.effective_web_player_configs().remove(key)?;
    let player_label = web.name.clone().unwrap_or_else(|| {
        if key == "default" {
            "Website defaults".to_string()
        } else {
            key.to_string()
        }
    });
    let patterns = web.match_patterns.clone();
    let player_config = web.into_player_config();
    let bus_name = format!("mprisence_web.{key}");
    let mut context = sample_context();
    context.player = player_label.clone();
    context.player_bus_name = bus_name.clone();
    context.metadata.url = patterns.iter().find_map(|pattern| {
        (!pattern.starts_with("re:") && !pattern.contains('*') && !pattern.contains('?'))
            .then(|| format!("https://{pattern}"))
    });
    let player_keys = vec![BRIDGE_CONFIG_KEY.to_string()];
    let allowed = config.is_source_allowed(&player_label, &bus_name, &player_keys, Some(key));
    Some(PreviewSelection {
        context,
        player_label,
        art_url: None,
        player_config,
        live: false,
        allowed,
        config_key: Some(BRIDGE_CONFIG_KEY.to_string()),
        web_player_key: Some(key.to_string()),
        matches: Vec::new(),
    })
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
    web_player_enabled: bool,
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
    let config = match config::load_config_from_file(config_path) {
        Ok(config) => config,
        Err(error) => return (400, "text/plain", error.to_string()),
    };
    let default = config::parse_config_str("").expect("bundled default config is valid");
    let settings = Settings {
        interval: config.interval,
        event_driven: config.event_driven,
        fallback_poll_interval: config.fallback_poll_interval,
        allowed_players: config.allowed_players.clone(),
        web_player_enabled: config.web_player_enabled,
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
struct PatchChange {
    path: Vec<String>,
    value: serde_json::Value,
}

#[derive(Deserialize)]
struct PatchRequest {
    path: Vec<String>,
    value: serde_json::Value,
    #[serde(default)]
    also: Vec<PatchChange>,
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
    let primary = PatchChange {
        path: request.path,
        value: request.value,
    };
    if let Err(message) = apply_patch(&mut doc, &primary) {
        return (400, "text/plain", message);
    }
    for change in &request.also {
        if change.path.is_empty() {
            return (400, "text/plain", "empty secondary path".to_string());
        }
        if let Err(message) = apply_patch(&mut doc, change) {
            return (400, "text/plain", message);
        }
    }
    // Reuse the validate-on-temp-file + atomic-rename save path.
    save_config(config_path, &doc.to_string())
}

fn apply_patch(doc: &mut toml_edit::DocumentMut, request: &PatchChange) -> Result<(), String> {
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
    fn index_raw_editor_has_dedicated_safe_loading_state() {
        assert!(INDEX_HTML.contains("<details id=\"rawEditor\">"));
        assert!(INDEX_HTML.contains("<button id=\"rawReload\" type=\"button\" disabled>"));
        assert!(INDEX_HTML.contains("<button id=\"rawSave\" disabled>"));
        assert!(INDEX_HTML.contains("$('rawEditor').addEventListener('toggle'"));
        assert!(!INDEX_HTML.contains("document.querySelector('details')"));
    }

    #[test]
    fn index_announces_save_and_connection_status() {
        assert!(INDEX_HTML.contains("id=\"saveStatus\" role=\"status\" aria-live=\"polite\""));
        assert!(INDEX_HTML.contains("id=\"offline\" role=\"alert\""));
        assert!(INDEX_HTML.contains("id=\"toast\" role=\"status\" aria-live=\"polite\""));
    }

    #[test]
    fn index_static_form_controls_have_programmatic_labels() {
        for id in [
            "pvPlayer",
            "t-details",
            "activityType",
            "timeShow",
            "eventDriven",
            "fallbackInterval",
            "interval",
            "allowedPlayers",
            "rawToml",
        ] {
            assert!(
                INDEX_HTML.contains(&format!("for=\"{id}\"")),
                "missing label for {id}"
            );
        }
    }

    #[test]
    fn index_groups_source_controls_by_user_intent() {
        for id in [
            "players",
            "settings-players",
            "settings-appearance",
            "settings-artwork",
            "settings-behavior",
            "settings-advanced",
            "playingRows",
            "webPolicyRows",
            "playerPolicyRows",
            "sourceFilter",
            "sourceDetail",
            "playerSearch",
            "localPlayersPanel",
            "webPlayersPanel",
            "previewColumn",
            "pvResolution",
            "variableSearch",
        ] {
            assert!(
                INDEX_HTML.contains(&format!("id=\"{id}\"")),
                "missing source control {id}"
            );
        }
        assert!(INDEX_HTML.contains("Websites with no web player rule are never shown"));
        assert!(INDEX_HTML.contains("Recognize web players"));
        assert!(INDEX_HTML.contains("id=\"webDisabledNote\""));
        assert!(INDEX_HTML.contains("id=\"webCatalog\" open"));
        assert!(INDEX_HTML.contains(">Players</button>"));
        assert!(INDEX_HTML.contains("id=\"playerPresetCatalog\" open"));
        assert!(INDEX_HTML.contains("This filter overrides the switches above"));
    }

    #[test]
    fn index_exposes_every_typed_config_area() {
        for id in [
            "activityType",
            "useContentType",
            "timeShow",
            "timeElapsed",
            "eventDriven",
            "fallbackInterval",
            "interval",
            "allowedPlayers",
            "imgbbKey",
            "mbMinScore",
            "catboxLitter",
            "catboxHours",
            "catboxHash",
            "imgbbExp",
            "coverDepth",
            "coverFiles",
            "providerList",
        ] {
            assert!(
                INDEX_HTML.contains(&format!("id=\"{id}\"")),
                "missing typed config control {id}"
            );
        }
        for key in [
            "name",
            "app_id",
            "icon",
            "show_icon",
            "allow_streaming",
            "status_display_type",
            "override_activity_type",
            "match_patterns",
            "title_suffix",
        ] {
            assert!(
                INDEX_HTML.contains(&format!("key: '{key}'")),
                "missing source override control {key}"
            );
        }
        assert!(INDEX_HTML.contains("'ignore'"));
        assert!(INDEX_HTML.contains("'ignore_unmatched'"));
        assert!(INDEX_HTML.contains("'match_pattern'"));
        assert!(INDEX_HTML.contains("id=\"rawToml\""));
        assert!(INDEX_HTML.contains("/api/config/validate"));
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
    fn raw_validation_reports_unknown_removed_and_deprecated_keys() {
        let body = "clear_on_pause = true\ndiscovery_interval = 1000\nwat = 1\n[template]\ndetail = \"x\"\n";
        let (status, _, payload) = route(
            &Method::Post,
            "/api/config/validate",
            body,
            &tmp_config_path("validate.toml"),
        );
        assert_eq!(status, 200);
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let warnings = parsed["warnings"].as_array().unwrap();
        assert!(warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("clear_on_pause")));
        assert!(warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("discovery_interval")));
        assert!(warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("Unknown setting: wat")));
        assert!(warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("template.detail")));
    }

    #[test]
    fn broken_config_is_not_silently_replaced_in_settings() {
        let path = tmp_config_path("broken-settings.toml");
        std::fs::write(&path, "[template\n").unwrap();
        let (status, _, message) = route(&Method::Get, "/api/settings", "", &path);
        assert_eq!(status, 400);
        assert!(!message.is_empty());
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
        assert_eq!(parsed["activity_type"], "listening");
        assert_eq!(parsed["status_display_type"], "state");
        assert!(parsed["context"].is_object());
        assert!(parsed["live"].is_boolean());
        assert!(parsed["ignored"].is_boolean());
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
        let (status, _, payload) = route(
            &Method::Get,
            "/api/settings",
            "",
            &tmp_config_path("set2.toml"),
        );
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
        let (status, ctype, payload) = route(
            &Method::Get,
            "/api/web_players",
            "",
            &tmp_config_path("web.toml"),
        );
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

    /// The toggle is a top-level scalar written into a file that already has
    /// tables — make sure toml_edit places it where it still parses back.
    #[test]
    fn patch_web_player_enabled_round_trips() {
        let path = tmp_config_path("webenabled.toml");
        std::fs::write(&path, "[template]\ndetails = \"{{{title}}}\"\n").unwrap();
        let body = serde_json::json!({ "path": ["web_player_enabled"], "value": false }).to_string();
        let (status, _, _) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 204);

        let reloaded = config::load_config_from_file(&path).expect("config should reload");
        assert!(!reloaded.web_player_enabled);
        assert!(reloaded
            .resolve_source(
                "Firefox",
                "firefox",
                Some("https://music.youtube.com/watch?v=x"),
                None,
            )
            .web_player_key
            .is_none());
    }

    #[test]
    fn patch_writes_array_value() {
        // match_patterns and provider order patch arrays; make sure that path
        // shape round-trips to a TOML array.
        let path = tmp_config_path("arraypatch.toml");
        let body = serde_json::json!({
            "path": ["web_player", "last_fm", "match_patterns"],
            "value": ["last.fm", "*.last.fm"]
        })
        .to_string();
        let (status, _, _) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 204);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("match_patterns = [\"last.fm\", \"*.last.fm\"]"));
    }

    #[test]
    fn patch_applies_related_changes_atomically() {
        let path = tmp_config_path("multi-patch.toml");
        let body = serde_json::json!({
            "path": ["web_player", "youtube", "match_patterns"],
            "value": ["music.youtube.com"],
            "also": [{ "path": ["web_player", "youtube", "match_pattern"], "value": "" }]
        })
        .to_string();
        let (status, _, _) = route(&Method::Patch, "/api/settings", &body, &path);
        assert_eq!(status, 204);
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("match_patterns = [\"music.youtube.com\"]"));
        assert!(saved.contains("match_pattern = \"\""));
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

    #[test]
    fn preview_uses_an_offline_local_preset() {
        let path = tmp_config_path("preview_local_preset.toml");
        std::fs::write(&path, "").unwrap();
        let response = render_preview(
            &path,
            &PreviewRequest {
                source_scope: Some("player".to_string()),
                source_key: Some("audacious".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(response.config_key.as_deref(), Some("audacious"));
        assert_ne!(response.player.as_deref(), Some("Sample Player"));
        assert!(!response.live);
    }

    #[test]
    fn preview_uses_a_preconfigured_web_player() {
        let path = tmp_config_path("preview_web_preset.toml");
        std::fs::write(&path, "").unwrap();
        let response = render_preview(
            &path,
            &PreviewRequest {
                source_scope: Some("web_player".to_string()),
                source_key: Some("youtube".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(response.config_key.as_deref(), Some(BRIDGE_CONFIG_KEY));
        assert_eq!(response.web_player_key.as_deref(), Some("youtube"));
        assert_ne!(response.player.as_deref(), Some("Sample Player"));
        assert!(!response.live);
    }

    fn fake_entry(bus: &str, status: &str, title: Option<&str>) -> PlayerEntry {
        let mut context = sample_context();
        context.status = Some(status.to_string());
        context.metadata.title = title.map(String::from);
        PlayerEntry {
            identity: bus.to_string(),
            player_bus_name: bus.to_string(),
            config_key: bus.to_string(),
            live: true,
            bundled: false,
            allowed: true,
            status: Some(status.to_string()),
            art_url: None,
            context,
            resolved: PlayerConfig::default(),
            effective: PlayerConfigLayer::default(),
            overrides: PlayerConfigLayer::default(),
            matches: Vec::new(),
            web_player_key: None,
        }
    }

    #[test]
    fn configured_only_adds_presets_default_and_offline_skipping_running() {
        // A running spotify plus a configured-but-not-running custom override.
        // user_player is only populated by the file loader, not parse_config_str.
        let path = tmp_config_path("configured_only.toml");
        std::fs::write(
            &path,
            "[player.vlc_media_player]\napp_id = \"123\"\n[player.spotify]\nignore = true\n",
        )
        .unwrap();
        let config = config::load_config_from_file(&path).unwrap();
        let mut entries = vec![fake_entry("spotify", "Playing", Some("x"))];
        append_configured_only(&mut entries, &config);
        let keys: Vec<&str> = entries.iter().map(|e| e.config_key.as_str()).collect();
        // Running spotify stays once, default and bundled presets are added, and
        // the not-running custom override shows up as a config-only row.
        assert_eq!(keys.iter().filter(|k| **k == "spotify").count(), 1);
        assert!(keys.contains(&"default"));
        assert!(keys.contains(&"audacious"));
        assert!(keys.contains(&"vlc_media_player"));
        let audacious = entries
            .iter()
            .find(|e| e.config_key == "audacious")
            .unwrap();
        assert!(audacious.bundled);
        assert!(!audacious.live);
        let vlc = entries
            .iter()
            .find(|e| e.config_key == "vlc_media_player")
            .unwrap();
        assert!(!vlc.live);
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
