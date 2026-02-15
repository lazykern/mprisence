use crate::{
    config::{
        get_config,
        schema::{ActivityType, StatusDisplayType, PlayerConfig},
    },
    error::Error,
    player::canonical_player_bus_name,
    utils::{format_playback_status_icon, normalize_player_identity},
};
use clap::{Parser, Subcommand};
use mpris::{PlaybackStatus, PlayerFinder};
use std::{cmp::Ordering, env, time::Duration};

#[derive(Parser)]
#[command(name = "mprisence")]
#[command(about = "Discord Rich Presence for MPRIS media players")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    Players {
        #[command(subcommand)]
        command: PlayersCommand,
    },
    Config,
    Version {
        #[command(subcommand)]
        command: Option<VersionCommand>,
    },
}

#[derive(Subcommand)]
pub enum PlayersCommand {
    List {
        #[arg(short, long)]
        detailed: bool,
    },
}

#[derive(Subcommand)]
pub enum VersionCommand {
    Validate { version: String },
}

impl Command {
    pub async fn execute(self) -> Result<(), Error> {
        match self {
            Command::Players { command } => match command {
                PlayersCommand::List { detailed } => {
                    let config = get_config();

                    let mut finder = PlayerFinder::new()?;
                    finder.set_player_timeout_ms(5000);

                    let players = finder.find_all()?;

                    if players.is_empty() {
                        println!("No MPRIS players found");
                        return Ok(());
                    }

                    let mut entries = Vec::with_capacity(players.len());
                    for mut player in players {
                        player.set_dbus_timeout_ms(5000);
                        let identity = player.identity().to_string();
                        let player_bus_name = canonical_player_bus_name(player.bus_name());
                        let id = normalize_player_identity(&identity);
                        let allowed = config.is_player_allowed(&identity, &player_bus_name);
                        let player_config = config.get_player_config(&identity, &player_bus_name);
                        let status = player.get_playback_status().ok();

                        let (title, artists, album, length) = match player.get_metadata() {
                            Ok(metadata) => {
                                let title = metadata.title().map(|value| value.to_string());
                                let artists = metadata.artists().map(|values| {
                                    values
                                        .into_iter()
                                        .map(|value| value.to_string())
                                        .collect::<Vec<_>>()
                                });
                                let album = metadata.album_name().map(|value| value.to_string());
                                let length = metadata
                                    .length()
                                    .map(|value| Duration::from_micros(value.as_micros() as u64));

                                (title, artists, album, length)
                            }
                            Err(_) => (None, None, None, None),
                        };

                        entries.push(PlayerDisplay {
                            id,
                            player_bus_name,
                            identity,
                            status,
                            title,
                            artists,
                            album,
                            length,
                            config: player_config,
                            allowed,
                        });
                    }

                    let total = entries.len();
                    let playing_count = entries
                        .iter()
                        .filter(|entry| {
                            entry.allowed && matches!(entry.status, Some(PlaybackStatus::Playing))
                        })
                        .count();
                    let paused_count = entries
                        .iter()
                        .filter(|entry| {
                            entry.allowed && matches!(entry.status, Some(PlaybackStatus::Paused))
                        })
                        .count();
                    let excluded_count = entries
                        .iter()
                        .filter(|entry| entry.config.ignore || !entry.allowed)
                        .count();

                    let divider = create_divider();

                    println!(
                        "\nMPRIS players: {} ({} playing, {} paused, {} excluded)",
                        total, playing_count, paused_count, excluded_count
                    );
                    println!("{}", divider);
                    for entry in &entries {
                        let summary_title = format_summary_title(entry);
                        println!(
                            "{} {} {} [{}]",
                            status_icon(entry.status.as_ref(), entry.config.ignore, entry.allowed),
                            format_cell(&entry.identity, NAME_COLUMN_WIDTH),
                            format_cell(&summary_title, TITLE_COLUMN_WIDTH),
                            summary_status_text(
                                entry.status.as_ref(),
                                entry.config.ignore,
                                entry.allowed
                            )
                        );
                    }

                    if detailed {
                        println!("\nDetails");
                        println!("{}", divider);

                        for (index, entry) in entries.iter().enumerate() {
                            println!(
                                "{} {}  [{}]",
                                status_icon(
                                    entry.status.as_ref(),
                                    entry.config.ignore,
                                    entry.allowed
                                ),
                                entry.identity,
                                detail_status_text(
                                    entry.status.as_ref(),
                                    entry.config.ignore,
                                    entry.allowed
                                )
                            );
                            if let Some(title) = &entry.title {
                                println!("  Title    : {}", title);
                            }
                            if let Some(artists) = &entry.artists {
                                if !artists.is_empty() {
                                    println!("  Artists  : {}", format_artists(artists));
                                }
                            }
                            if let Some(album) = &entry.album {
                                println!("  Album    : {}", album);
                            }
                            if let Some(length) = entry.length {
                                println!("  Length   : {}", format_track_length(length));
                            }
                            println!(
                                "  Presence : {}",
                                format_presence(&entry.config, entry.allowed)
                            );
                            println!("  ID       : {}", entry.id);
                            println!("  Bus Name : {}", entry.player_bus_name);

                            if index + 1 < entries.len() {
                                println!();
                            }
                        }
                    }
                }
            },
            Command::Config => {
                let config = get_config();
                let config_path = config.config_path();

                println!("\n{} )", config_path.display());
                println!("{}", create_divider());

                println!("\nGeneral");
                print_key_value("interval", format!("{} ms", config.interval()));
                print_key_value("clear_on_pause", format_bool(config.clear_on_pause()));
                print_key_value("config_path", config_path.display());
                print_key_value("allowed_players", format_vector(&config.allowed_players()));

                let activity_config = config.activity_type_config();
                println!("\nActivity");
                print_key_value(
                    "default_type",
                    format_activity_type(Some(activity_config.default)),
                );
                print_key_value(
                    "use_content_type",
                    format_bool(activity_config.use_content_type),
                );

                let time_config = config.time_config();
                println!("\nTime Display");
                print_key_value("show_time", format_bool(time_config.show));
                print_key_value("as_elapsed", format_bool(time_config.as_elapsed));

                let cover_config = config.cover_config();
                println!("\nCover Art");
                print_key_value("providers", format_vector(&cover_config.provider.provider));
                print_key_value(
                    "imgbb_expiration",
                    format!("{} s", cover_config.provider.imgbb.expiration),
                );
                let key_state = if cover_config.provider.imgbb.api_key.is_some() {
                    "set"
                } else {
                    "not set"
                };
                print_key_value("imgbb_api_key", key_state);
                print_key_value("mb_min_score", cover_config.provider.musicbrainz.min_score);
                print_key_value("local_search_depth", cover_config.local_search_depth);
                print_key_value("file_names", format_vector(&cover_config.file_names));

                let template_config = config.template_config();
                println!("\nTemplates");
                print_key_value("detail", template_config.detail.as_ref());
                print_key_value("state", template_config.state.as_ref());
                print_key_value("large_text", template_config.large_text.as_ref());
                print_key_value("small_text", template_config.small_text.as_ref());

                let mut player_configs: Vec<(String, PlayerConfig)> =
                    config.player_configs().into_iter().collect();
                player_configs.sort_by(|a, b| compare_player_keys(a.0.as_str(), b.0.as_str()));

                println!("\nOverrides Detail");
                println!("{}", create_divider());
                if player_configs.is_empty() {
                    println!("  (none)");
                } else {
                    for (index, (identity, cfg)) in player_configs.iter().enumerate() {
                        let display = player_config_display_name(identity);
                        println!("{} {}", player_config_icon(cfg.ignore), display);
                        print_nested_key_value("app_id", &cfg.app_id, 4);
                        print_nested_key_value(
                            "allow_streaming",
                            format_bool(cfg.allow_streaming),
                            4,
                        );
                        print_nested_key_value("status_disp_type", format_status_disp_type(cfg.status_disp_type), 4);
                        print_nested_key_value("show_icon", format_bool(cfg.show_icon), 4);
                        print_nested_key_value("ignore", format_bool(cfg.ignore), 4);
                        print_nested_key_value("icon", &cfg.icon, 4);
                        if let Some(activity_type) = cfg.override_activity_type {
                            print_nested_key_value(
                                "activity_type",
                                format_activity_type(Some(activity_type)),
                                4,
                            );
                        }

                        if index + 1 < player_configs.len() {
                            println!();
                        }
                    }
                }
            }
            Command::Version { command } => match command {
                Some(VersionCommand::Validate { version }) => {
                    match crate::utils::validate_version(&version) {
                        Ok(normalized) => {
                            println!("Version '{}' is valid", normalized);
                            println!("Normalized: {}", normalized);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    println!("mprisence {}", env!("CARGO_PKG_VERSION"));
                }
            },
        }
        Ok(())
    }
}

const NAME_COLUMN_WIDTH: usize = 32;
const TITLE_COLUMN_WIDTH: usize = 36;
const DIVIDER_WIDTH: usize = 56;
const CONFIG_KEY_WIDTH: usize = 18;

struct PlayerDisplay {
    id: String,
    player_bus_name: String,
    identity: String,
    status: Option<PlaybackStatus>,
    title: Option<String>,
    artists: Option<Vec<String>>,
    album: Option<String>,
    length: Option<Duration>,
    config: PlayerConfig,
    allowed: bool,
}

fn create_divider() -> String {
    "─".repeat(DIVIDER_WIDTH)
}

fn status_icon(status: Option<&PlaybackStatus>, ignored: bool, allowed: bool) -> &'static str {
    if ignored || !allowed {
        "✖"
    } else if let Some(status) = status {
        format_playback_status_icon(*status)
    } else {
        "?"
    }
}

fn playback_status_word(status: Option<&PlaybackStatus>) -> Option<&'static str> {
    match status {
        Some(PlaybackStatus::Playing) => Some("playing"),
        Some(PlaybackStatus::Paused) => Some("paused"),
        Some(PlaybackStatus::Stopped) => Some("stopped"),
        None => None,
    }
}

fn summary_status_text(status: Option<&PlaybackStatus>, ignored: bool, allowed: bool) -> String {
    detail_status_text(status, ignored, allowed)
}

fn detail_status_text(status: Option<&PlaybackStatus>, ignored: bool, allowed: bool) -> String {
    let mut parts = Vec::new();
    if let Some(word) = playback_status_word(status) {
        parts.push(word.to_string());
    }
    if ignored {
        parts.push("ignored".to_string());
    }
    if !allowed {
        parts.push("disallowed".to_string());
    }

    if parts.is_empty() {
        "unknown".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_cell(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let truncated = truncate_value(value, width);
    format!("{:<width$}", truncated, width = width)
}

fn format_summary_title(entry: &PlayerDisplay) -> String {
    match (&entry.title, &entry.album) {
        (Some(title), Some(album)) if !title.is_empty() && !album.is_empty() => {
            format!("{} — {}", title, album)
        }
        (Some(title), _) if !title.is_empty() => title.clone(),
        (None, Some(album)) if !album.is_empty() => album.clone(),
        _ => "—".to_string(),
    }
}

fn truncate_value(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let mut truncated: String = value.chars().take(width - 3).collect();
    truncated.push_str("...");
    truncated
}

fn format_artists(artists: &[String]) -> String {
    artists.join(" • ")
}

fn format_track_length(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

fn format_presence(config: &PlayerConfig, allowed: bool) -> String {
    if !allowed {
        "disallowed by allowed_players".to_string()
    } else if config.ignore {
        format!("ignored (app_id = {})", config.app_id)
    } else {
        format!("enabled (allow_streaming = {})", config.allow_streaming)
    }
}

fn print_key_value(key: &str, value: impl std::fmt::Display) {
    print_key_value_with_indent(2, key, value);
}

fn print_nested_key_value(key: &str, value: impl std::fmt::Display, indent: usize) {
    print_key_value_with_indent(indent, key, value);
}

fn print_key_value_with_indent(indent: usize, key: &str, value: impl std::fmt::Display) {
    let indent_space = " ".repeat(indent);
    println!(
        "{}{: <width$}: {}",
        indent_space,
        key,
        value,
        width = CONFIG_KEY_WIDTH
    );
}

fn format_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn format_vector(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        let joined = values
            .iter()
            .map(|value| format!("\"{}\"", value))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{}]", joined)
    }
}

fn format_activity_type(activity: Option<ActivityType>) -> String {
    activity
        .map(|value| format!("{:?}", value).to_lowercase())
        .unwrap_or_else(|| "—".to_string())
}

fn format_status_disp_type(disp_type: StatusDisplayType) -> String {
    format!("{:?}", disp_type).to_lowercase()
}

fn compare_player_keys(a: &str, b: &str) -> Ordering {
    match (a == "default", b == "default") {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.cmp(b),
    }
}

fn player_config_icon(ignored: bool) -> &'static str {
    if ignored {
        "✖"
    } else {
        "▶"
    }
}

fn player_config_display_name(identity: &str) -> String {
    if identity == "default" {
        "default".to_string()
    } else {
        normalize_player_identity(identity)
    }
}
