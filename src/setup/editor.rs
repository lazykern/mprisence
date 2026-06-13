use inquire::Text;
use std::collections::BTreeMap;
use std::path::Path;
use toml_edit::{Array, Value};

use crate::config;
use crate::config::schema::{
    ActivityType, Config, PlayerConfig, PlayerConfigLayer, StatusDisplayType, WebPlayerConfig,
    WebPlayerConfigLayer,
};
use crate::error::Error;
use crate::setup::effective;
use crate::setup::fields;
use crate::setup::hints;
use crate::setup::patch::{apply_edit, ConfigEdit, ConfigPatch, TablePatch};
use crate::setup::ui::{self, confirm_save, prompt_select, BACK_LABEL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySource {
    BundledOnly,
    BundledWithUserOverride,
    UserOnly,
    LiveOnly,
}

pub fn player_source(config: &Config, key: &str) -> EntrySource {
    let in_bundled = config.bundled_player.contains_key(key);
    let in_user = config.user_player.contains_key(key);
    match (in_bundled, in_user) {
        (true, true) => EntrySource::BundledWithUserOverride,
        (true, false) => EntrySource::BundledOnly,
        (false, true) => EntrySource::UserOnly,
        (false, false) => EntrySource::LiveOnly,
    }
}

pub fn web_source(config: &Config, key: &str) -> EntrySource {
    let in_bundled = config.bundled_web_player.contains_key(key);
    let in_user = config.user_web_player.contains_key(key);
    match (in_bundled, in_user) {
        (true, true) => EntrySource::BundledWithUserOverride,
        (true, false) => EntrySource::BundledOnly,
        (false, true) => EntrySource::UserOnly,
        (false, false) => EntrySource::LiveOnly,
    }
}

pub fn can_delete_entry(source: EntrySource) -> bool {
    matches!(
        source,
        EntrySource::UserOnly | EntrySource::BundledWithUserOverride
    )
}

pub fn can_reset_entry(source: EntrySource) -> bool {
    source == EntrySource::BundledWithUserOverride
}

pub enum EditOutcome {
    Cancelled,
    Remove,
    Save(TablePatch),
}

pub fn save_table_patch(
    config_path: &std::path::Path,
    section: &str,
    key: &str,
    patch: &TablePatch,
) -> Result<(), Error> {
    if patch.is_empty() {
        return Ok(());
    }
    let mut cfg_patch = ConfigPatch::default();
    cfg_patch
        .tables
        .insert(format!("{section}.{key}"), patch.clone());
    apply_edit(config_path, &ConfigEdit {
        patch: cfg_patch,
        ..Default::default()
    })
}

pub fn delete_entry(
    config_path: &std::path::Path,
    section: &str,
    key: &str,
) -> Result<bool, Error> {
    let mut edit = ConfigEdit::default();
    edit.remove_table(&format!("{section}.{key}"));
    apply_edit(config_path, &edit)?;
    Ok(true)
}

pub fn edit_player_layer(
    config_path: &Path,
    key: &str,
    fallback_effective: &PlayerConfig,
) -> Result<EditOutcome, Error> {
    let (_, config) = config::load_merged_config(Some(config_path))?;
    let effective = config
        .effective_player_configs()
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback_effective.clone());
    let source = player_source(&config, key);
    let initial = config
        .user_player
        .get(key)
        .cloned()
        .unwrap_or_default();
    let mut draft = initial.clone();
    let mut dirty = false;
    let is_default_entry = key == "default";

    let global_activity = config.activity_type.default;

    loop {
        ui::redraw(&["Settings", "Players", key], &format!("Edit · {key}"))?;
        print_player_summary(key, &draft, &effective, global_activity);
        let status_effective = draft
            .status_display_type
            .unwrap_or(effective.status_display_type);
        let status_menu = effective::format_menu(
            "Status display type",
            effective::status_display_type_str(status_effective),
            effective::layer_source(&draft.status_display_type),
        );
        let activity_menu = {
            let display = effective::activity_display(
                draft.override_activity_type,
                effective.override_activity_type,
                global_activity,
            );
            effective::format_menu("Activity type override", display.label, display.source)
        };
        let mut options = Vec::new();
        if !is_default_entry {
            options.push(format_ignore_label(&draft, &effective));
        }
        options.extend([
            "Display name".to_string(),
            "Discord app ID".to_string(),
            "Icon URL".to_string(),
            effective::format_menu_bool("Show icon in presence", draft.show_icon, effective.show_icon),
            effective::format_menu_bool(
                "Allow streaming",
                draft.allow_streaming,
                effective.allow_streaming,
            ),
            status_menu,
            activity_menu,
        ]);
        if can_reset_entry(source) {
            options.push("Reset user overrides".to_string());
        }
        if can_delete_entry(source) {
            options.push("Delete user entry".to_string());
        }
        options.push(BACK_LABEL.to_string());

        let choice = match prompt_select("", options)? {
            Some(value) => value,
            None => break,
        };

        if choice == BACK_LABEL {
            break;
        }
        if choice == "Reset user overrides" {
            ui::redraw(&["Settings", "Players", key], "Reset overrides")?;
            if confirm_save("Remove your overrides and revert to bundled defaults?")? {
                return Ok(EditOutcome::Remove);
            }
            continue;
        }
        if choice == "Delete user entry" {
            ui::redraw(&["Settings", "Players", key], "Delete entry")?;
            if confirm_save("Delete this entry from your config file?")? {
                return Ok(EditOutcome::Remove);
            }
            continue;
        }

        let changed = match choice.as_str() {
            s if s.starts_with("Ignore") => {
                ui::redraw(&["Settings", "Players", key], "ignore")?;
                edit_ignore(&mut draft, &effective)?
            }
            "Display name" => {
                ui::redraw(&["Settings", "Players", key], "Display name override")?;
                let effective_name = effective.name.as_deref().unwrap_or(key);
                edit_optional_string(
                    "Display name override",
                    &hints::player_name_help(
                        draft.name.as_deref().or(effective.name.as_deref()),
                        key,
                    ),
                    &mut draft.name,
                    Some(effective_name),
                )?
            }
            "Discord app ID" => {
                ui::redraw(&["Settings", "Players", key], "Discord app ID")?;
                edit_required_string_with_effective(
                    "Discord app ID",
                    &hints::app_id_help(
                        draft
                            .app_id
                            .as_deref()
                            .unwrap_or(effective.app_id.as_str()),
                    ),
                    &mut draft.app_id,
                    &effective.app_id,
                )?
            }
            "Icon URL" => {
                ui::redraw(&["Settings", "Players", key], "Icon URL")?;
                edit_required_string_with_effective(
                    "Icon URL",
                    &hints::icon_help(draft.icon.as_deref().unwrap_or(effective.icon.as_str())),
                    &mut draft.icon,
                    &effective.icon,
                )?
            }
            s if s.starts_with("Show icon") => {
                ui::redraw(&["Settings", "Players", key], "Show icon")?;
                edit_optional_bool_with_help(
                    "Show icon in presence?",
                    &hints::show_icon_help(draft.show_icon.unwrap_or(effective.show_icon)),
                    &mut draft.show_icon,
                    effective.show_icon,
                )?
            }
            s if s.starts_with("Allow streaming") => {
                ui::redraw(&["Settings", "Players", key], "Allow streaming")?;
                edit_optional_bool_with_help(
                    "Allow streaming activity?",
                    &hints::allow_streaming_help(
                        draft.allow_streaming.unwrap_or(effective.allow_streaming),
                    ),
                    &mut draft.allow_streaming,
                    effective.allow_streaming,
                )?
            }
            s if s.starts_with("Status display type") => {
                ui::redraw(&["Settings", "Players", key], "Status display type")?;
                edit_status_display_type(
                    &mut draft.status_display_type,
                    effective.status_display_type,
                )?
            }
            s if s.starts_with("Activity type override") => {
                ui::redraw(&["Settings", "Players", key], "Activity type")?;
                edit_activity_type(
                    &mut draft.override_activity_type,
                    effective.override_activity_type,
                    global_activity,
                )?
            }
            _ => false,
        };
        dirty |= changed;
    }

    if !dirty {
        return Ok(EditOutcome::Cancelled);
    }

    ui::redraw(&["Settings", "Players", key], "Save changes")?;
    if !confirm_save("Save player changes?")? {
        return Ok(EditOutcome::Cancelled);
    }

    if is_default_entry {
        draft.ignore = None;
    }

    let patch = player_layer_diff(&initial, &draft);
    if patch.is_empty() {
        return Ok(EditOutcome::Cancelled);
    }

    Ok(EditOutcome::Save(patch))
}

pub fn edit_web_layer(
    config_path: &Path,
    key: &str,
    fallback_effective: &WebPlayerConfig,
    require_patterns: bool,
) -> Result<EditOutcome, Error> {
    let (_, config) = config::load_merged_config(Some(config_path))?;
    let effective = config
        .effective_web_player_configs()
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback_effective.clone());
    let source = web_source(&config, key);
    let initial = config
        .user_web_player
        .get(key)
        .cloned()
        .unwrap_or_default();
    let mut draft = initial.clone();
    let mut dirty = false;
    let is_default_entry = key == "default";

    let global_activity = config.activity_type.default;
    let web_runtime_defaults = effective::web_runtime(&WebPlayerConfigLayer::default(), &effective);

    loop {
        ui::redraw(&["Settings", "Web sites", key], &format!("Edit · {key}"))?;
        print_web_summary(key, &draft, &effective, global_activity);
        let (status_effective, _) = effective::resolved_web_status(&draft, &effective);
        let status_menu = effective::format_menu(
            "Status display type",
            effective::status_display_type_str(status_effective),
            effective::web_option_source(&draft.status_display_type, &effective.status_display_type),
        );
        let activity_menu = {
            let display = effective::activity_display(
                draft.override_activity_type,
                effective.override_activity_type,
                global_activity,
            );
            effective::format_menu("Activity type override", display.label, display.source)
        };
        let mut options = Vec::new();
        if !is_default_entry {
            options.push(format_ignore_label_web(&draft, &effective));
        }
        options.extend([
            "Match host(s)".to_string(),
            "Title suffix".to_string(),
            "Display name".to_string(),
            "Discord app ID".to_string(),
            "Icon URL".to_string(),
            effective::format_menu_bool_web(
                "Show icon in presence",
                draft.show_icon,
                effective.show_icon,
                web_runtime_defaults.show_icon,
            ),
            effective::format_menu_bool_web(
                "Allow streaming",
                draft.allow_streaming,
                effective.allow_streaming,
                web_runtime_defaults.allow_streaming,
            ),
            status_menu,
            activity_menu,
        ]);
        if can_reset_entry(source) {
            options.push("Reset user overrides".to_string());
        }
        if can_delete_entry(source) {
            options.push("Delete user entry".to_string());
        }
        options.push(BACK_LABEL.to_string());

        let choice = match prompt_select("", options)? {
            Some(value) => value,
            None => break,
        };

        if choice == BACK_LABEL {
            break;
        }
        if choice == "Reset user overrides" {
            ui::redraw(&["Settings", "Web sites", key], "Reset overrides")?;
            if confirm_save("Remove your overrides and revert to bundled defaults?")? {
                return Ok(EditOutcome::Remove);
            }
            continue;
        }
        if choice == "Delete user entry" {
            ui::redraw(&["Settings", "Web sites", key], "Delete entry")?;
            if confirm_save("Delete this entry from your config file?")? {
                return Ok(EditOutcome::Remove);
            }
            continue;
        }

        let changed = match choice.as_str() {
            s if s.starts_with("Ignore") => {
                ui::redraw(&["Settings", "Web sites", key], "ignore")?;
                edit_ignore_web(&mut draft, &effective)?
            }
            "Match host(s)" => {
                ui::redraw(&["Settings", "Web sites", key], "Match host(s)")?;
                edit_match_patterns(&mut draft.match_patterns, &effective.match_patterns)?
            }
            "Title suffix" => {
                ui::redraw(&["Settings", "Web sites", key], "Title suffix")?;
                edit_optional_string(
                    "Title suffix",
                    &hints::title_suffix_help(
                        draft
                            .title_suffix
                            .as_deref()
                            .or(effective.title_suffix.as_deref()),
                    ),
                    &mut draft.title_suffix,
                    effective.title_suffix.as_deref(),
                )?
            }
            "Display name" => {
                ui::redraw(&["Settings", "Web sites", key], "Display name override")?;
                edit_optional_string(
                    "Display name override",
                    &hints::web_name_help(
                        draft.name.as_deref().or(effective.name.as_deref()),
                        key,
                    ),
                    &mut draft.name,
                    Some(effective.name.as_deref().unwrap_or(key)),
                )?
            }
            "Discord app ID" => {
                ui::redraw(&["Settings", "Web sites", key], "Discord app ID")?;
                edit_optional_string(
                    "Discord app ID",
                    &hints::app_id_optional_help(
                        draft.app_id.as_deref().or(effective.app_id.as_deref()),
                    ),
                    &mut draft.app_id,
                    effective
                        .app_id
                        .as_deref()
                        .or(Some(web_runtime_defaults.app_id.as_str())),
                )?
            }
            "Icon URL" => {
                ui::redraw(&["Settings", "Web sites", key], "Icon URL")?;
                edit_optional_string(
                    "Icon URL",
                    &hints::icon_optional_help(
                        draft.icon.as_deref().or(effective.icon.as_deref()),
                    ),
                    &mut draft.icon,
                    effective
                        .icon
                        .as_deref()
                        .or(Some(web_runtime_defaults.icon.as_str())),
                )?
            }
            s if s.starts_with("Show icon") => {
                ui::redraw(&["Settings", "Web sites", key], "Show icon")?;
                edit_optional_bool_with_help(
                    "Show icon in presence?",
                    &hints::show_icon_help(
                        draft
                            .show_icon
                            .unwrap_or(effective.show_icon.unwrap_or(web_runtime_defaults.show_icon)),
                    ),
                    &mut draft.show_icon,
                    effective
                        .show_icon
                        .unwrap_or(web_runtime_defaults.show_icon),
                )?
            }
            s if s.starts_with("Allow streaming") => {
                ui::redraw(&["Settings", "Web sites", key], "Allow streaming")?;
                edit_optional_bool_with_help(
                    "Allow streaming activity?",
                    &hints::allow_streaming_help(
                        draft.allow_streaming.unwrap_or(
                            effective
                                .allow_streaming
                                .unwrap_or(web_runtime_defaults.allow_streaming),
                        ),
                    ),
                    &mut draft.allow_streaming,
                    effective
                        .allow_streaming
                        .unwrap_or(web_runtime_defaults.allow_streaming),
                )?
            }
            s if s.starts_with("Status display type") => {
                ui::redraw(&["Settings", "Web sites", key], "Status display type")?;
                let (status_effective, _) = effective::resolved_web_status(&draft, &effective);
                edit_status_display_type(&mut draft.status_display_type, status_effective)?
            }
            s if s.starts_with("Activity type override") => {
                ui::redraw(&["Settings", "Web sites", key], "Activity type")?;
                edit_activity_type(
                    &mut draft.override_activity_type,
                    effective.override_activity_type,
                    global_activity,
                )?
            }
            _ => false,
        };
        dirty |= changed;
    }

    if require_patterns && draft.match_patterns.as_ref().is_none_or(|v| v.is_empty()) {
        ui::print_dim("Match host(s) required for a custom web site.");
        edit_match_patterns(&mut draft.match_patterns, &effective.match_patterns)?;
        dirty = true;
    }

    if !dirty {
        return Ok(EditOutcome::Cancelled);
    }

    ui::redraw(&["Settings", "Web sites", key], "Save changes")?;
    if !confirm_save("Save web site changes?")? {
        return Ok(EditOutcome::Cancelled);
    }

    if is_default_entry {
        draft.ignore = None;
    }

    let patch = web_layer_diff(&initial, &draft);
    if patch.is_empty() {
        return Ok(EditOutcome::Cancelled);
    }

    Ok(EditOutcome::Save(patch))
}

fn print_player_summary(
    key: &str,
    draft: &PlayerConfigLayer,
    effective: &PlayerConfig,
    global_activity: ActivityType,
) {
    pub_print_player_summary(key, draft, effective, global_activity, false);
}

pub fn print_player_summary_compact(
    key: &str,
    draft: &PlayerConfigLayer,
    effective: &PlayerConfig,
    global_activity: ActivityType,
) {
    pub_print_player_summary(key, draft, effective, global_activity, true);
}

fn pub_print_player_summary(
    key: &str,
    draft: &PlayerConfigLayer,
    effective: &PlayerConfig,
    global_activity: ActivityType,
    compact: bool,
) {
    let runtime = effective::player_runtime(draft, effective);
    let name_shown = runtime.name.as_deref().unwrap_or(key);
    ui::print_dim(&effective::format_summary(
        "display name",
        name_shown,
        effective::layer_source(&draft.name),
    ));
    if !compact {
        if key == "default" {
            ui::print_dim("  hide unknown players: use Defaults → Toggle hide unknown local players");
        } else {
            let ignore = resolved_player_ignore(draft, effective);
            ui::print_dim(&effective::format_summary(
                "ignore",
                ignore,
                effective::layer_source(&draft.ignore),
            ));
        }
    }
    ui::print_dim(&effective::format_summary(
        "app_id",
        runtime.app_id.as_str(),
        effective::layer_source(&draft.app_id),
    ));
    if !compact {
        ui::print_dim(&effective::format_summary(
            "icon",
            truncate_for_summary(runtime.icon.as_str()),
            effective::layer_source(&draft.icon),
        ));
    }
    ui::print_dim(&effective::format_summary(
        "show_icon",
        runtime.show_icon,
        effective::layer_source(&draft.show_icon),
    ));
    if !compact {
        ui::print_dim(&effective::format_summary(
            "allow_streaming",
            runtime.allow_streaming,
            effective::layer_source(&draft.allow_streaming),
        ));
    }
    ui::print_dim(&effective::format_summary(
        "status_display_type",
        effective::status_display_type_str(runtime.status_display_type),
        effective::layer_source(&draft.status_display_type),
    ));
    if !compact {
        let activity = effective::activity_display(
            draft.override_activity_type,
            effective.override_activity_type,
            global_activity,
        );
        ui::print_dim(&effective::format_summary(
            "override_activity_type",
            activity.label,
            activity.source,
        ));
    }
    println!();
}

fn print_web_summary(
    key: &str,
    draft: &WebPlayerConfigLayer,
    effective: &WebPlayerConfig,
    global_activity: ActivityType,
) {
    pub_print_web_summary(key, draft, effective, global_activity, false);
}

pub fn print_web_summary_compact(
    key: &str,
    draft: &WebPlayerConfigLayer,
    effective: &WebPlayerConfig,
    global_activity: ActivityType,
) {
    pub_print_web_summary(key, draft, effective, global_activity, true);
}

fn pub_print_web_summary(
    key: &str,
    draft: &WebPlayerConfigLayer,
    effective: &WebPlayerConfig,
    global_activity: ActivityType,
    compact: bool,
) {
    let runtime = effective::web_runtime(draft, effective);
    let merged = effective::web_merged(draft, effective);
    let hosts = draft
        .match_patterns
        .as_ref()
        .or(Some(&effective.match_patterns))
        .map(|values| values.join(", "))
        .unwrap_or_else(|| "(none)".to_string());
    if !compact {
        ui::print_dim(&effective::format_summary(
            "match_patterns",
            hosts,
            effective::layer_source(&draft.match_patterns),
        ));
    }
    let name_shown = runtime.name.as_deref().unwrap_or(key);
    ui::print_dim(&effective::format_summary(
        "display name",
        name_shown,
        effective::web_option_source(&draft.name, &merged.name),
    ));
    if !compact {
        if key == "default" {
            ui::print_dim("  hide unknown URLs: use Defaults → Toggle hide unknown web URLs");
        } else {
            let ignore = resolved_web_ignore(draft, effective);
            ui::print_dim(&effective::format_summary(
                "ignore",
                ignore,
                effective::layer_source(&draft.ignore),
            ));
        }
        let suffix = draft
            .title_suffix
            .as_deref()
            .or(effective.title_suffix.as_deref())
            .unwrap_or("(none)");
        ui::print_dim(&effective::format_summary(
            "title_suffix",
            suffix,
            effective::web_option_source(&draft.title_suffix, &effective.title_suffix),
        ));
    }
    ui::print_dim(&effective::format_summary(
        "app_id",
        runtime.app_id.as_str(),
        effective::web_option_source(&draft.app_id, &merged.app_id),
    ));
    if !compact {
        ui::print_dim(&effective::format_summary(
            "icon",
            truncate_for_summary(runtime.icon.as_str()),
            effective::web_option_source(&draft.icon, &merged.icon),
        ));
    }
    ui::print_dim(&effective::format_summary(
        "show_icon",
        runtime.show_icon,
        effective::web_option_source(&draft.show_icon, &merged.show_icon),
    ));
    if !compact {
        ui::print_dim(&effective::format_summary(
            "allow_streaming",
            runtime.allow_streaming,
            effective::web_option_source(&draft.allow_streaming, &merged.allow_streaming),
        ));
    }
    ui::print_dim(&effective::format_summary(
        "status_display_type",
        effective::status_display_type_str(runtime.status_display_type),
        effective::web_option_source(&draft.status_display_type, &merged.status_display_type),
    ));
    if !compact {
        let activity = effective::activity_display(
            draft.override_activity_type,
            effective.override_activity_type,
            global_activity,
        );
        ui::print_dim(&effective::format_summary(
            "override_activity_type",
            activity.label,
            activity.source,
        ));
    }
    println!();
}

fn resolved_player_ignore(layer: &PlayerConfigLayer, effective: &PlayerConfig) -> bool {
    layer.ignore.unwrap_or(effective.ignore)
}

fn resolved_web_ignore(layer: &WebPlayerConfigLayer, effective: &WebPlayerConfig) -> bool {
    layer.ignore.unwrap_or(effective.ignore)
}

fn format_ignore_label(layer: &PlayerConfigLayer, effective: &PlayerConfig) -> String {
    let ignore = resolved_player_ignore(layer, effective);
    format!("Ignore ({ignore})")
}

fn format_ignore_label_web(layer: &WebPlayerConfigLayer, effective: &WebPlayerConfig) -> String {
    let ignore = resolved_web_ignore(layer, effective);
    format!("Ignore ({ignore})")
}

fn player_layer_diff(before: &PlayerConfigLayer, after: &PlayerConfigLayer) -> TablePatch {
    let mut entries = BTreeMap::new();
    let mut removed_keys = Vec::new();

    diff_option_string("name", &before.name, &after.name, &mut entries, &mut removed_keys);
    diff_option_bool("ignore", before.ignore, after.ignore, &mut entries, &mut removed_keys);
    diff_option_string("app_id", &before.app_id, &after.app_id, &mut entries, &mut removed_keys);
    diff_option_string("icon", &before.icon, &after.icon, &mut entries, &mut removed_keys);
    diff_option_bool(
        "show_icon",
        before.show_icon,
        after.show_icon,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_bool(
        "allow_streaming",
        before.allow_streaming,
        after.allow_streaming,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_status_display(
        "status_display_type",
        before.status_display_type,
        after.status_display_type,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_activity_type(
        "override_activity_type",
        before.override_activity_type,
        after.override_activity_type,
        &mut entries,
        &mut removed_keys,
    );

    TablePatch {
        entries,
        removed_keys,
    }
}

fn web_layer_diff(before: &WebPlayerConfigLayer, after: &WebPlayerConfigLayer) -> TablePatch {
    let mut entries = BTreeMap::new();
    let mut removed_keys = Vec::new();

    diff_option_string_vec(
        "match_patterns",
        &before.match_patterns,
        &after.match_patterns,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_string(
        "title_suffix",
        &before.title_suffix,
        &after.title_suffix,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_string("name", &before.name, &after.name, &mut entries, &mut removed_keys);
    diff_option_bool("ignore", before.ignore, after.ignore, &mut entries, &mut removed_keys);
    diff_option_string("app_id", &before.app_id, &after.app_id, &mut entries, &mut removed_keys);
    diff_option_string("icon", &before.icon, &after.icon, &mut entries, &mut removed_keys);
    diff_option_bool(
        "show_icon",
        before.show_icon,
        after.show_icon,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_bool(
        "allow_streaming",
        before.allow_streaming,
        after.allow_streaming,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_status_display(
        "status_display_type",
        before.status_display_type,
        after.status_display_type,
        &mut entries,
        &mut removed_keys,
    );
    diff_option_activity_type(
        "override_activity_type",
        before.override_activity_type,
        after.override_activity_type,
        &mut entries,
        &mut removed_keys,
    );

    TablePatch {
        entries,
        removed_keys,
    }
}

fn diff_option_string(
    key: &str,
    before: &Option<String>,
    after: &Option<String>,
    entries: &mut BTreeMap<String, Value>,
    removed_keys: &mut Vec<String>,
) {
    if before == after {
        return;
    }
    match after {
        Some(value) => {
            entries.insert(key.to_string(), Value::from(value.as_str()));
        }
        None => removed_keys.push(key.to_string()),
    }
}

fn diff_option_bool(
    key: &str,
    before: Option<bool>,
    after: Option<bool>,
    entries: &mut BTreeMap<String, Value>,
    removed_keys: &mut Vec<String>,
) {
    if before == after {
        return;
    }
    match after {
        Some(value) => {
            entries.insert(key.to_string(), Value::from(value));
        }
        None => removed_keys.push(key.to_string()),
    }
}

fn diff_option_status_display(
    key: &str,
    before: Option<StatusDisplayType>,
    after: Option<StatusDisplayType>,
    entries: &mut BTreeMap<String, Value>,
    removed_keys: &mut Vec<String>,
) {
    if before == after {
        return;
    }
    match after {
        Some(value) => {
            entries.insert(key.to_string(), Value::from(effective::status_display_type_str(value)));
        }
        None => removed_keys.push(key.to_string()),
    }
}

fn diff_option_activity_type(
    key: &str,
    before: Option<ActivityType>,
    after: Option<ActivityType>,
    entries: &mut BTreeMap<String, Value>,
    removed_keys: &mut Vec<String>,
) {
    if before == after {
        return;
    }
    match after {
        Some(value) => {
            entries.insert(key.to_string(), Value::from(effective::activity_type_str(value)));
        }
        None => removed_keys.push(key.to_string()),
    }
}

fn diff_option_string_vec(
    key: &str,
    before: &Option<Vec<String>>,
    after: &Option<Vec<String>>,
    entries: &mut BTreeMap<String, Value>,
    removed_keys: &mut Vec<String>,
) {
    if before == after {
        return;
    }
    match after {
        Some(values) => {
            let mut array = Array::new();
            for value in values {
                array.push(Value::from(value.as_str()));
            }
            entries.insert(key.to_string(), Value::Array(array));
        }
        None => removed_keys.push(key.to_string()),
    }
}

fn edit_ignore(layer: &mut PlayerConfigLayer, effective: &PlayerConfig) -> Result<bool, Error> {
    let current = resolved_player_ignore(layer, effective);
    let Some(next) = fields::prompt_bool_with_help(
        "ignore (hide from Discord presence)?",
        current,
        &hints::ignore_help(current),
    )? else {
        return Ok(false);
    };
    let new_ignore = Some(next);
    if layer.ignore == new_ignore {
        return Ok(false);
    }
    layer.ignore = new_ignore;
    Ok(true)
}

fn edit_ignore_web(
    layer: &mut WebPlayerConfigLayer,
    effective: &WebPlayerConfig,
) -> Result<bool, Error> {
    let current = resolved_web_ignore(layer, effective);
    let Some(next) = fields::prompt_bool_with_help(
        "ignore (hide from Discord presence)?",
        current,
        &hints::ignore_help(current),
    )? else {
        return Ok(false);
    };
    let new_ignore = Some(next);
    if layer.ignore == new_ignore {
        return Ok(false);
    }
    layer.ignore = new_ignore;
    Ok(true)
}

fn edit_required_string_with_effective(
    message: &str,
    help: &str,
    target: &mut Option<String>,
    effective_fallback: &str,
) -> Result<bool, Error> {
    let default = target.as_deref().unwrap_or(effective_fallback);
    let mut prompt = Text::new(message).with_default(default);
    if !help.is_empty() {
        prompt = prompt.with_help_message(help);
    }
    let value = match prompt.prompt_skippable() {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(false),
        Err(inquire::InquireError::OperationCanceled) => return Ok(false),
        Err(err) => {
            return Err(Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )))
        }
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        ui::print_dim("Value cannot be empty.");
        return Ok(false);
    }
    let next = Some(trimmed.to_string());
    if target == &next {
        return Ok(false);
    }
    *target = next;
    Ok(true)
}

fn edit_optional_string(
    message: &str,
    help: &str,
    target: &mut Option<String>,
    effective_default: Option<&str>,
) -> Result<bool, Error> {
    let default = target
        .as_deref()
        .or(effective_default)
        .unwrap_or("");
    let mut prompt = Text::new(message).with_default(default);
    if !help.is_empty() {
        prompt = prompt.with_help_message(help);
    }
    let value = match prompt.prompt_skippable() {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(false),
        Err(inquire::InquireError::OperationCanceled) => return Ok(false),
        Err(err) => {
            return Err(Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )))
        }
    };
    let next = if value.trim().is_empty() {
        None
    } else {
        Some(value.trim().to_string())
    };
    if target == &next {
        return Ok(false);
    }
    *target = next;
    Ok(true)
}

fn edit_match_patterns(
    target: &mut Option<Vec<String>>,
    effective: &[String],
) -> Result<bool, Error> {
    let current = target
        .as_ref()
        .map(|values| values.join(", "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| effective.join(", "));
    let help = if current.is_empty() {
        "URL host to match (e.g. music.youtube.com). Required for web sites.".to_string()
    } else {
        hints::match_pattern_help(&current)
    };
    let value = match Text::new("Match host(s), comma-separated")
        .with_default(&current)
        .with_help_message(&help)
        .prompt_skippable()
    {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(false),
        Err(inquire::InquireError::OperationCanceled) => return Ok(false),
        Err(err) => {
            return Err(Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )))
        }
    };
    let hosts: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if hosts.is_empty() {
        println!("At least one host required.");
        return Ok(false);
    }
    let next = Some(hosts);
    if target == &next {
        return Ok(false);
    }
    *target = next;
    Ok(true)
}

fn edit_optional_bool_with_help(
    message: &str,
    help: &str,
    target: &mut Option<bool>,
    effective_fallback: bool,
) -> Result<bool, Error> {
    let current = target.unwrap_or(effective_fallback);
    let Some(next) = fields::prompt_bool_with_help(message, current, help)? else {
        return Ok(false);
    };
    let new_value = Some(next);
    if target == &new_value {
        return Ok(false);
    }
    *target = new_value;
    Ok(true)
}

fn status_option_label(value: StatusDisplayType, current: StatusDisplayType) -> String {
    let label = effective::status_display_type_str(value);
    if value == current {
        format!("→ {label}")
    } else {
        label.to_string()
    }
}

fn activity_option_label(value: ActivityType, current: ActivityType) -> String {
    let label = effective::activity_type_str(value);
    if value == current {
        format!("→ {label}")
    } else {
        label.to_string()
    }
}

fn edit_status_display_type(
    target: &mut Option<StatusDisplayType>,
    effective: StatusDisplayType,
) -> Result<bool, Error> {
    let current = target.unwrap_or(effective);
    let options = vec![
        status_option_label(StatusDisplayType::Name, current),
        status_option_label(StatusDisplayType::State, current),
        status_option_label(StatusDisplayType::Details, current),
        "clear override".to_string(),
        BACK_LABEL.to_string(),
    ];
    let choice = match prompt_select("Status display type", options)? {
        Some(value) => value,
        None => return Ok(false),
    };
    if choice == BACK_LABEL {
        return Ok(false);
    }
    let next = if choice == "clear override" {
        None
    } else {
        Some(effective::parse_status_display_type(&choice))
    };
    if target == &next {
        return Ok(false);
    }
    *target = next;
    Ok(true)
}

fn edit_activity_type(
    target: &mut Option<ActivityType>,
    effective: Option<ActivityType>,
    global: ActivityType,
) -> Result<bool, Error> {
    let current = target.or(effective).unwrap_or(global);
    let options = vec![
        activity_option_label(ActivityType::Listening, current),
        activity_option_label(ActivityType::Watching, current),
        activity_option_label(ActivityType::Playing, current),
        activity_option_label(ActivityType::Competing, current),
        "clear override".to_string(),
        BACK_LABEL.to_string(),
    ];
    let choice = match prompt_select("Activity type override", options)? {
        Some(value) => value,
        None => return Ok(false),
    };
    if choice == BACK_LABEL {
        return Ok(false);
    }
    let next = if choice == "clear override" {
        None
    } else {
        Some(effective::parse_activity_type(&choice))
    };
    if target == &next {
        return Ok(false);
    }
    *target = next;
    Ok(true)
}

fn truncate_for_summary(value: &str) -> String {
    if value.chars().count() <= 48 {
        value.to_string()
    } else {
        format!("{}…", value.chars().take(48).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{ActivityType, StatusDisplayType};

    #[test]
    fn player_layer_diff_empty_when_unchanged() {
        let layer = PlayerConfigLayer {
            app_id: Some("123".to_string()),
            ..Default::default()
        };
        let patch = player_layer_diff(&layer, &layer);
        assert!(patch.is_empty());
    }

    #[test]
    fn player_layer_diff_records_changed_fields() {
        let before = PlayerConfigLayer::default();
        let after = PlayerConfigLayer {
            app_id: Some("999".to_string()),
            show_icon: Some(true),
            ..Default::default()
        };
        let patch = player_layer_diff(&before, &after);
        assert_eq!(patch.entries.get("app_id").and_then(|v| v.as_str()), Some("999"));
        assert_eq!(patch.entries.get("show_icon").and_then(|v| v.as_bool()), Some(true));
        assert!(patch.removed_keys.is_empty());
    }

    #[test]
    fn player_layer_diff_clears_optional_fields() {
        let before = PlayerConfigLayer {
            name: Some("Custom".to_string()),
            override_activity_type: Some(ActivityType::Watching),
            ..Default::default()
        };
        let after = PlayerConfigLayer::default();
        let patch = player_layer_diff(&before, &after);
        assert!(patch.entries.is_empty());
        assert!(patch.removed_keys.contains(&"name".to_string()));
        assert!(patch
            .removed_keys
            .contains(&"override_activity_type".to_string()));
    }

    #[test]
    fn web_layer_diff_records_match_patterns() {
        let before = WebPlayerConfigLayer::default();
        let after = WebPlayerConfigLayer {
            match_patterns: Some(vec!["music.youtube.com".to_string()]),
            ignore: Some(false),
            ..Default::default()
        };
        let patch = web_layer_diff(&before, &after);
        assert_eq!(
            patch.entries.get("match_patterns").and_then(|v| v.as_array()).map(|a| a.len()),
            Some(1)
        );
        assert_eq!(patch.entries.get("ignore").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn web_layer_diff_empty_when_unchanged() {
        let layer = WebPlayerConfigLayer {
            title_suffix: Some(" | YTM".to_string()),
            status_display_type: Some(StatusDisplayType::Details),
            ..Default::default()
        };
        let patch = web_layer_diff(&layer, &layer);
        assert!(patch.is_empty());
    }
}
