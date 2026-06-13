use std::collections::{BTreeMap, HashSet};

use inquire::{InquireError, MultiSelect};

use crate::config;
use crate::config::schema::Config;
use crate::error::Error;
use crate::setup::fields;
use crate::setup::global::save_global_edit;
use crate::setup::patch::{apply_edit, ConfigEdit, ConfigPatch};
use crate::setup::ui::{self, prompt_select, BACK_LABEL};
use crate::utils::normalize_player_identity;

const MIN_INTERVAL_MS: u64 = 500;

struct PatternCandidate {
    pattern: String,
    label: String,
}

pub fn run(config_path: &std::path::Path) -> Result<(), Error> {
    loop {
        let (_, config) = config::load_merged_config(Some(config_path))?;
        ui::redraw(&["Settings", "Discovery"], "Discovery")?;

        if config.event_driven {
            ui::print_dim(&format!(
                "Mode: event-driven (fallback poll every {}ms)",
                config.fallback_poll_interval
            ));
        } else {
            ui::print_dim(&format!("Mode: poll-only (every {}ms)", config.interval));
        }
        ui::print_dim(&allowed_players_summary(&config));
        println!();

        let choice = match prompt_select(
            "",
            vec![
                "Edit discovery mode and intervals".to_string(),
                "Edit allowed players".to_string(),
                "Reset discovery to bundled defaults".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Edit discovery mode and intervals" => edit_discovery(config_path, &config)?,
            "Edit allowed players" => edit_allowed_players(config_path, &config)?,
            "Reset discovery to bundled defaults" => reset_discovery(config_path)?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn allowed_players_summary(config: &Config) -> String {
    if config.allowed_players.is_empty() {
        "allowed_players: allow all".to_string()
    } else {
        let preview: Vec<_> = config.allowed_players.iter().take(3).cloned().collect();
        let suffix = if config.allowed_players.len() > 3 {
            format!(", +{}", config.allowed_players.len() - 3)
        } else {
            String::new()
        };
        format!(
            "allowed_players: {} pattern(s) ({}{suffix})",
            config.allowed_players.len(),
            preview.join(", ")
        )
    }
}

fn edit_discovery(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    let Some(event_driven) = fields::prompt_bool_with_help(
        "Use event-driven discovery?",
        config.event_driven,
        "Subscribe to MPRIS signals. false = poll-only mode.",
    )? else {
        return Ok(());
    };

    let mut patch = ConfigPatch::default();
    patch.set_top_level_bool("event_driven", event_driven);

    if event_driven {
        let Some(fallback) = prompt_interval_ms(
            "Fallback poll interval (ms)",
            config.fallback_poll_interval,
            "Safety poll when events are quiet.",
        )? else {
            return Ok(());
        };
        patch.set_top_level_u64("fallback_poll_interval", fallback);
    } else {
        let Some(interval) = prompt_interval_ms(
            "Poll interval (ms)",
            config.interval,
            "How often to scan MPRIS players in poll-only mode.",
        )? else {
            return Ok(());
        };
        patch.set_top_level_u64("interval", interval);
    }

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save discovery settings?")?;
    Ok(())
}

fn edit_allowed_players(
    config_path: &std::path::Path,
    config: &Config,
) -> Result<(), Error> {
    ui::redraw(
        &["Settings", "Discovery", "Allowed players"],
        "Allowed players",
    )?;
    ui::print_dim("Checked = allowed. Empty selection = allow all players.");
    ui::print_dim("Use exact identity, wildcard (*mpd*), or re:regex patterns.");

    let candidates = collect_pattern_candidates(config)?;
    if candidates.is_empty() {
        ui::print_dim("No player patterns available.");
        return Ok(());
    }

    let current: HashSet<&str> = config
        .allowed_players
        .iter()
        .map(String::as_str)
        .collect();

    let defaults: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| current.contains(c.pattern.as_str()))
        .map(|(idx, _)| idx)
        .collect();

    let labels: Vec<String> = candidates.iter().map(|c| c.label.clone()).collect();

    let selected = match MultiSelect::new("", labels.clone())
        .with_default(&defaults)
        .prompt()
    {
        Ok(values) => values,
        Err(InquireError::OperationCanceled) => return Ok(()),
        Err(err) => {
            return Err(Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )))
        }
    };

    let selected_labels: HashSet<&str> = selected.iter().map(String::as_str).collect();
    let mut patterns: Vec<String> = candidates
        .iter()
        .filter(|c| selected_labels.contains(c.label.as_str()))
        .map(|c| c.pattern.clone())
        .collect();

    loop {
        ui::redraw(
            &["Settings", "Discovery", "Allowed players"],
            "Custom pattern",
        )?;
        let Some(raw) = fields::prompt_optional_text_with_help(
            "Add custom pattern (optional)",
            None,
            "Wildcard or re:regex. Leave blank and press Enter to finish.",
        )? else {
            break;
        };
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            break;
        }
        let normalized = normalize_player_identity(&trimmed);
        if !patterns.contains(&normalized) {
            patterns.push(normalized);
        }
    }

    patterns.sort();
    patterns.dedup();

    let mut current_sorted = config.allowed_players.clone();
    current_sorted.sort();
    current_sorted.dedup();

    if patterns == current_sorted {
        ui::print_dim("No allowed_players changes.");
        return Ok(());
    }

    let mut edit = ConfigEdit::default();
    if patterns.is_empty() {
        if !config.allowed_players.is_empty() {
            edit.remove_top_level("allowed_players");
        }
    } else {
        edit.patch
            .set_top_level_string_array("allowed_players", &patterns);
    }

    let _ = save_global_edit(config_path, &edit, "Save allowed players?")?;
    Ok(())
}

fn collect_pattern_candidates(config: &Config) -> Result<Vec<PatternCandidate>, Error> {
    let mut by_pattern: BTreeMap<String, PatternCandidate> = BTreeMap::new();

    for (key, cfg) in config.effective_player_configs() {
        if key == "default" {
            continue;
        }
        let label = cfg
            .name
            .as_deref()
            .map(|name| format!("{name} ({key})"))
            .unwrap_or_else(|| key.clone());
        by_pattern.insert(
            key.clone(),
            PatternCandidate {
                pattern: key,
                label,
            },
        );
    }

    for (identity, key) in super::mpris::collect_live_player_identities()? {
        by_pattern.entry(key.clone()).or_insert(PatternCandidate {
            pattern: key,
            label: format!("{identity} (live)"),
        });
    }

    for pattern in &config.allowed_players {
        by_pattern.entry(pattern.clone()).or_insert(PatternCandidate {
            pattern: pattern.clone(),
            label: pattern.clone(),
        });
    }

    Ok(by_pattern.into_values().collect())
}

fn prompt_interval_ms(label: &str, current: u64, help: &str) -> Result<Option<u64>, Error> {
    loop {
        let Some(raw) = fields::prompt_text_with_help(label, &current.to_string(), help)? else {
            return Ok(None);
        };
        let parsed: u64 = match raw.trim().parse() {
            Ok(value) => value,
            Err(_) => {
                ui::print_dim("Enter a positive number (milliseconds).");
                continue;
            }
        };
        if parsed < MIN_INTERVAL_MS {
            ui::print_dim(&format!("Minimum interval is {MIN_INTERVAL_MS}ms."));
            continue;
        }
        return Ok(Some(parsed));
    }
}

fn reset_discovery(config_path: &std::path::Path) -> Result<(), Error> {
    if !ui::confirm_save(
        "Remove user discovery overrides (event_driven, interval, fallback_poll_interval, allowed_players)?",
    )? {
        return Ok(());
    }
    let mut edit = ConfigEdit::default();
    for key in [
        "event_driven",
        "interval",
        "fallback_poll_interval",
        "allowed_players",
    ] {
        edit.remove_top_level(key);
    }
    apply_edit(config_path, &edit)?;
    Ok(())
}
