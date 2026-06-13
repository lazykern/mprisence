use std::collections::BTreeMap;

use crate::config::{self, schema::{Config, PlayerConfig}};
use crate::error::Error;
use crate::player::{is_mprisence_web_bridge_bus, is_playerctld_no_active_error};
use crate::setup::editor::{delete_entry, edit_player_layer, save_table_patch, EditOutcome};
use crate::setup::quick_toggle::{run_quick_toggle, ToggleEntry};
use crate::setup::ui::{self, prompt_select, BACK_LABEL};
use crate::utils::normalize_player_identity;
use mpris::PlayerFinder;

pub fn run(config_path: &std::path::Path) -> Result<(), Error> {
    loop {
        let (_, config) = config::load_merged_config(Some(config_path))?;
        ui::redraw(&["Settings", "Players"], "Players")?;
        let choice = match prompt_select(
            "",
            vec![
                "Quick show/hide (ignore)".to_string(),
                "Edit player…".to_string(),
                "Add custom player…".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Quick show/hide (ignore)" => {
                let entries = collect_player_entries(&config)?;
                run_quick_toggle(
                    &["Settings", "Players"],
                    "player",
                    "player",
                    entries,
                    config_path,
                )?;
            }
            "Edit player…" => run_edit_player(&config, config_path)?,
            "Add custom player…" => run_add_custom(&config, config_path)?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn run_edit_player(config: &Config, config_path: &std::path::Path) -> Result<(), Error> {
    let entries = collect_player_entries(config)?;
    if entries.is_empty() {
        ui::print_dim("No players to edit.");
        return Ok(());
    }

    ui::redraw(&["Settings", "Players", "Edit"], "Select player")?;
    let labels: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
    let label = match prompt_select("", labels)? {
        Some(value) => value,
        None => return Ok(()),
    };

    let entry = entries
        .into_iter()
        .find(|e| e.label == label)
        .expect("selected player should exist");

    apply_player_edit_outcome(config_path, &entry.key, &config.effective_player_configs().get(&entry.key).cloned().unwrap_or_else(|| {
        PlayerConfig { ignore: true, ..PlayerConfig::default() }
    }))
}

fn run_add_custom(config: &Config, config_path: &std::path::Path) -> Result<(), Error> {
    ui::redraw(&["Settings", "Players", "Add custom"], "Add player")?;
    let source = match prompt_select(
        "",
        vec![
            "Live MPRIS player".to_string(),
            "Manual identity".to_string(),
            BACK_LABEL.to_string(),
        ],
    )? {
        Some(value) => value,
        None => return Ok(()),
    };

    if source == BACK_LABEL {
        return Ok(());
    }

    let key = if source == "Live MPRIS player" {
        match pick_live_player_key(config)? {
            Some(key) => key,
            None => return Ok(()),
        }
    } else {
        ui::redraw(&["Settings", "Players", "Add custom"], "Manual identity")?;
        let raw = ui::prompt_text(
            "Player identity (exact, wildcard, or re:regex)",
            "my_player",
            "MPRIS identity or pattern used in [player.*]",
        )?;
        normalize_player_identity(&raw)
    };

    if key.is_empty() {
        return Err(Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "player identity cannot be empty",
        )));
    }

    if config.effective_player_configs().contains_key(&key) {
        ui::print_dim(&format!(
            "Player '{key}' already exists — use Edit player instead."
        ));
        return Ok(());
    }

    let mut base = PlayerConfig::default();
    base.ignore = false;

    apply_player_edit_outcome(config_path, &key, &base)
}

fn apply_player_edit_outcome(
    config_path: &std::path::Path,
    key: &str,
    fallback_effective: &PlayerConfig,
) -> Result<(), Error> {
    match edit_player_layer(config_path, key, fallback_effective)? {
        EditOutcome::Cancelled => Ok(()),
        EditOutcome::Remove => {
            if delete_entry(config_path, "player", key)? {
                ui::print_dim(&format!(
                    "Removed [player.{key}] from {}",
                    config_path.display()
                ));
            }
            Ok(())
        }
        EditOutcome::Save(patch) => {
            save_table_patch(config_path, "player", key, &patch)?;
            ui::print_dim(&format!("Saved to {}", config_path.display()));
            Ok(())
        }
    }
}

fn pick_live_player_key(config: &Config) -> Result<Option<String>, Error> {
    let mut choices = Vec::new();
    for (identity, key) in super::mpris::collect_live_player_identities()? {
        if config.effective_player_configs().contains_key(&key) {
            continue;
        }
        choices.push(format!("{identity} → {key}"));
    }

    if choices.is_empty() {
        ui::print_dim("No new live MPRIS players found.");
        return Ok(None);
    }

    ui::redraw(&["Settings", "Players", "Add custom"], "Live player")?;
    let choice = match prompt_select("", choices)? {
        Some(value) => value,
        None => return Ok(None),
    };

    Ok(choice
        .split(" → ")
        .nth(1)
        .map(str::to_string))
}

fn collect_player_entries(config: &Config) -> Result<Vec<ToggleEntry>, Error> {
    let effective = config.effective_player_configs();
    let mut by_key: BTreeMap<String, ToggleEntry> = BTreeMap::new();

    for (key, cfg) in effective {
        if key == "default" {
            continue;
        }
        by_key.insert(
            key.clone(),
            ToggleEntry {
                label: player_label(&key, cfg.name.as_deref()),
                key,
                enabled: !cfg.ignore,
            },
        );
    }

    if let Ok(mut finder) = PlayerFinder::new() {
        finder.set_player_timeout_ms(3000);
        if let Ok(iter) = finder.iter_players() {
            for player in iter {
                match player {
                    Ok(player) => {
                        if is_mprisence_web_bridge_bus(player.bus_name()) {
                            continue;
                        }
                        let identity = player.identity();
                        let id = normalize_player_identity(identity);
                        if by_key.contains_key(&id) {
                            continue;
                        }
                        by_key.insert(
                            id.clone(),
                            ToggleEntry {
                                key: id,
                                label: format!("{identity} (live)"),
                                enabled: false,
                            },
                        );
                    }
                    Err(err) if is_playerctld_no_active_error(&err) => {}
                    Err(err) => return Err(err.into()),
                }
            }
        }
    }

    Ok(by_key.into_values().collect())
}

fn player_label(key: &str, name: Option<&str>) -> String {
    if let Some(name) = name {
        format!("{name} ({key})")
    } else {
        key.to_string()
    }
}
