use crate::config::{self, schema::Config};
use crate::error::Error;
use crate::setup::editor::{
    delete_entry, edit_player_layer, edit_web_layer, print_player_summary_compact,
    print_web_summary_compact, save_table_patch, EditOutcome,
};
use crate::setup::global::save_global_edit;
use crate::setup::patch::{apply_edit, ConfigEdit, ConfigPatch};
use crate::setup::ui::{self, prompt_select, BACK_LABEL};

pub fn run(config_path: &std::path::Path) -> Result<(), Error> {
    loop {
        let (_, config) = config::load_merged_config(Some(config_path))?;
        ui::redraw(&["Settings", "Defaults"], "Defaults")?;

        let hide_players = config.ignore_unmatched_players();
        let hide_web = config.ignore_unmatched_web_players();
        let player_defaults = config.default_player_config();
        let web_defaults = config.default_web_player_config();
        let global_activity = config.activity_type.default;
        let player_draft = config
            .user_player
            .get("default")
            .cloned()
            .unwrap_or_default();
        let web_draft = config
            .user_web_player
            .get("default")
            .cloned()
            .unwrap_or_default();

        ui::print_dim(&format!(
            "Unknown local players: {}",
            if hide_players { "hidden" } else { "allowed" }
        ));
        ui::print_dim(&format!(
            "Unknown web URLs: {}",
            if hide_web { "hidden" } else { "allowed" }
        ));
        ui::print_dim("Player defaults:");
        print_player_summary_compact("default", &player_draft, &player_defaults, global_activity);
        ui::print_dim("Web defaults:");
        print_web_summary_compact("default", &web_draft, &web_defaults, global_activity);

        let choice = match prompt_select(
            "",
            vec![
                "Edit player defaults…".to_string(),
                "Edit web defaults…".to_string(),
                "Toggle hide unknown local players".to_string(),
                "Toggle hide unknown web URLs".to_string(),
                "Reset defaults section to bundled".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Edit player defaults…" => edit_player_defaults(config_path, &config)?,
            "Edit web defaults…" => edit_web_defaults(config_path, &config)?,
            "Toggle hide unknown local players" => {
                toggle_ignore_unmatched(config_path, "player", !hide_players)?;
            }
            "Toggle hide unknown web URLs" => {
                toggle_ignore_unmatched(config_path, "web_player", !hide_web)?;
            }
            "Reset defaults section to bundled" => reset_defaults(config_path)?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn edit_player_defaults(config_path: &std::path::Path, config: &Config) -> Result<(), Error> {
    let effective = config.default_player_config();
    apply_player_default_outcome(config_path, &effective)
}

fn edit_web_defaults(config_path: &std::path::Path, config: &Config) -> Result<(), Error> {
    let effective = config.default_web_player_config();
    apply_web_default_outcome(config_path, &effective)
}

fn apply_player_default_outcome(
    config_path: &std::path::Path,
    fallback_effective: &config::schema::PlayerConfig,
) -> Result<(), Error> {
    match edit_player_layer(config_path, "default", fallback_effective)? {
        EditOutcome::Cancelled => Ok(()),
        EditOutcome::Remove => {
            if delete_entry(config_path, "player", "default")? {
                ui::print_dim(&format!(
                    "Removed [player.default] from {}",
                    config_path.display()
                ));
            }
            Ok(())
        }
        EditOutcome::Save(patch) => {
            save_table_patch(config_path, "player", "default", &patch)?;
            ui::print_dim(&format!("Saved to {}", config_path.display()));
            Ok(())
        }
    }
}

fn apply_web_default_outcome(
    config_path: &std::path::Path,
    fallback_effective: &config::schema::WebPlayerConfig,
) -> Result<(), Error> {
    match edit_web_layer(config_path, "default", fallback_effective, false)? {
        EditOutcome::Cancelled => Ok(()),
        EditOutcome::Remove => {
            if delete_entry(config_path, "web_player", "default")? {
                ui::print_dim(&format!(
                    "Removed [web_player.default] from {}",
                    config_path.display()
                ));
            }
            Ok(())
        }
        EditOutcome::Save(patch) => {
            save_table_patch(config_path, "web_player", "default", &patch)?;
            ui::print_dim(&format!("Saved to {}", config_path.display()));
            Ok(())
        }
    }
}

fn toggle_ignore_unmatched(
    config_path: &std::path::Path,
    section: &str,
    value: bool,
) -> Result<(), Error> {
    let mut patch = ConfigPatch::default();
    patch.set_table_bool(&format!("{section}.default"), "ignore_unmatched", value);
    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(
        config_path,
        &edit,
        &format!("Save {section}.default.ignore_unmatched = {value}?"),
    )?;
    Ok(())
}

fn reset_defaults(config_path: &std::path::Path) -> Result<(), Error> {
    if !ui::confirm_save("Remove user [player.default] and [web_player.default] overrides?")? {
        return Ok(());
    }
    let mut edit = ConfigEdit::default();
    edit.remove_table("player.default");
    edit.remove_table("web_player.default");
    apply_edit(config_path, &edit)?;
    ui::print_dim(&format!("Reset defaults in {}", config_path.display()));
    Ok(())
}
