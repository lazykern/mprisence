use std::collections::BTreeMap;

use crate::config::{self, schema::{Config, WebPlayerConfig}};
use crate::error::Error;
use crate::setup::editor::{delete_entry, edit_web_layer, save_table_patch, EditOutcome};
use crate::setup::quick_toggle::{run_quick_toggle, ToggleEntry};
use crate::setup::ui::{self, prompt_select, BACK_LABEL};

pub fn run(config_path: &std::path::Path) -> Result<(), Error> {
    loop {
        let (_, config) = config::load_merged_config(Some(config_path))?;
        ui::redraw(&["Settings", "Web sites"], "Web sites")?;
        let choice = match prompt_select(
            "",
            vec![
                "Quick show/hide (ignore)".to_string(),
                "Edit site…".to_string(),
                "Add custom site…".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Quick show/hide (ignore)" => {
                let entries = collect_web_entries(&config);
                run_quick_toggle(
                    &["Settings", "Web sites"],
                    "web_player",
                    "web site",
                    entries,
                    config_path,
                )?;
            }
            "Edit site…" => run_edit_site(&config, config_path)?,
            "Add custom site…" => run_add_custom(&config, config_path)?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn run_edit_site(config: &Config, config_path: &std::path::Path) -> Result<(), Error> {
    let entries = collect_web_entries(config);
    if entries.is_empty() {
        ui::print_dim("No web sites to edit.");
        return Ok(());
    }

    ui::redraw(&["Settings", "Web sites", "Edit"], "Select site")?;
    let labels: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
    let label = match prompt_select("", labels)? {
        Some(value) => value,
        None => return Ok(()),
    };

    let entry = entries
        .into_iter()
        .find(|e| e.label == label)
        .expect("selected site should exist");

    let effective = config
        .effective_web_player_configs()
        .get(&entry.key)
        .cloned()
        .unwrap_or_default();

    apply_web_edit_outcome(config_path, &entry.key, &effective, false)
}

fn run_add_custom(config: &Config, config_path: &std::path::Path) -> Result<(), Error> {
    ui::redraw(&["Settings", "Web sites", "Add custom"], "New site")?;
    let key = ui::prompt_text(
        "Config key (e.g. my_streaming_site)",
        "my_site",
        "Table name under [web_player.*] in config",
    )?;
    let key = normalize_web_key(&key);
    if key.is_empty() || key == "default" {
        return Err(Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid web site key",
        )));
    }

    if config.effective_web_player_configs().contains_key(&key) {
        ui::print_dim(&format!("Site '{key}' already exists — use Edit site instead."));
        return Ok(());
    }

    let mut base = WebPlayerConfig::default();
    base.ignore = false;

    apply_web_edit_outcome(config_path, &key, &base, true)
}

fn apply_web_edit_outcome(
    config_path: &std::path::Path,
    key: &str,
    fallback_effective: &WebPlayerConfig,
    require_patterns: bool,
) -> Result<(), Error> {
    match edit_web_layer(config_path, key, fallback_effective, require_patterns)? {
        EditOutcome::Cancelled => Ok(()),
        EditOutcome::Remove => {
            if delete_entry(config_path, "web_player", key)? {
                ui::print_dim(&format!(
                    "Removed [web_player.{key}] from {}",
                    config_path.display()
                ));
            }
            Ok(())
        }
        EditOutcome::Save(patch) => {
            save_table_patch(config_path, "web_player", key, &patch)?;
            ui::print_dim(&format!("Saved to {}", config_path.display()));
            Ok(())
        }
    }
}

fn collect_web_entries(config: &Config) -> Vec<ToggleEntry> {
    let effective = config.effective_web_player_configs();
    let mut by_key: BTreeMap<String, ToggleEntry> = BTreeMap::new();

    for (key, cfg) in effective {
        if key == "default" {
            continue;
        }
        let host = cfg
            .match_patterns
            .first()
            .map(|s| s.as_str())
            .unwrap_or("?");
        let name = cfg.name.as_deref().unwrap_or(&key);
        let label = format!("{name} ({host})");
        by_key.insert(
            key.clone(),
            ToggleEntry {
                key,
                label,
                enabled: !cfg.ignore,
            },
        );
    }

    by_key.into_values().collect()
}

pub(crate) fn normalize_web_key(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::normalize_web_key;

    #[test]
    fn normalize_web_key_trims_and_lowercases() {
        assert_eq!(normalize_web_key("  My Site  "), "my_site");
    }

    #[test]
    fn normalize_web_key_replaces_spaces_with_underscores() {
        assert_eq!(normalize_web_key("my streaming site"), "my_streaming_site");
    }

    #[test]
    fn normalize_web_key_empty_after_trim() {
        assert_eq!(normalize_web_key("   "), "");
    }
}
