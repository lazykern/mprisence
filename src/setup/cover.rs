use crate::config;
use crate::error::Error;
use crate::setup::fields;
use crate::setup::global::save_global_edit;
use crate::setup::patch::{apply_edit, ConfigEdit, ConfigPatch};
use crate::setup::ui::{self, prompt_select, BACK_LABEL};

const KNOWN_PROVIDERS: &[&str] = &["catbox", "musicbrainz", "imgbb"];
const LITTER_HOURS: &[u8] = &[1, 12, 24, 72];

pub fn run(config_path: &std::path::Path) -> Result<(), Error> {
    loop {
        let (_, config) = config::load_merged_config(Some(config_path))?;
        ui::redraw(&["Settings", "Cover art"], "Cover art")?;

        ui::print_dim(&format!(
            "Providers: {}",
            config.cover.provider.provider.join(" → ")
        ));
        ui::print_dim(&format!(
            "Local search depth: {}",
            config.cover.local_search_depth
        ));
        let imgbb = if config.cover.provider.imgbb.api_key.is_some() {
            "set"
        } else {
            "not set"
        };
        ui::print_dim(&format!("ImgBB API key: {imgbb}"));
        println!();

        let choice = match prompt_select(
            "",
            vec![
                "Local search".to_string(),
                "Provider order".to_string(),
                "MusicBrainz settings".to_string(),
                "Catbox settings".to_string(),
                "ImgBB settings".to_string(),
                "Reset cover art to bundled defaults".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Local search" => edit_local_search(config_path, &config)?,
            "Provider order" => edit_provider_order(config_path, &config)?,
            "MusicBrainz settings" => edit_musicbrainz(config_path, &config)?,
            "Catbox settings" => edit_catbox(config_path, &config)?,
            "ImgBB settings" => edit_imgbb(config_path, &config)?,
            "Reset cover art to bundled defaults" => reset_cover(config_path)?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn edit_local_search(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(&["Settings", "Cover art", "Local search"], "Local search")?;

    let names_default = config.cover.file_names.join(", ");
    let Some(names_raw) = fields::prompt_text_with_help(
        "Cover file names (comma-separated)",
        &names_default,
        "Search order for cover.jpg, folder.png, etc. in media directories.",
    )? else {
        return Ok(());
    };
    let file_names: Vec<String> = names_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let Some(depth_raw) = fields::prompt_text_with_help(
        "Local search depth",
        &config.cover.local_search_depth.to_string(),
        "How many parent directories to search upward (0 = same dir only).",
    )? else {
        return Ok(());
    };
    let depth: usize = depth_raw.trim().parse().map_err(|_| {
        Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid search depth",
        ))
    })?;

    let mut patch = ConfigPatch::default();
    patch.set_table_array(
        "cover",
        "file_names",
        &file_names.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    );
    patch.set_table_value(
        "cover",
        "local_search_depth",
        toml_edit::Value::from(depth as i64),
    );

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save local cover search settings?")?;
    Ok(())
}

fn edit_provider_order(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(&["Settings", "Cover art", "Providers"], "Provider order")?;
    ui::print_dim(&format!(
        "Current: {}",
        config.cover.provider.provider.join(" → ")
    ));

    let current = config.cover.provider.provider.join(", ");
    let help = format!(
        "Comma-separated preference order (first = tried first). Known: {}. imgbb requires an API key.",
        KNOWN_PROVIDERS.join(", ")
    );
    let Some(raw) = fields::prompt_text_with_help("Cover art providers", &current, &help)? else {
        return Ok(());
    };

    let providers = match parse_provider_order(&raw) {
        Ok(values) => values,
        Err(message) => {
            ui::print_dim(&message);
            return Ok(());
        }
    };

    if providers == config.cover.provider.provider {
        return Ok(());
    }

    let mut patch = ConfigPatch::default();
    patch.set_table_array(
        "cover.provider",
        "provider",
        &providers.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    );

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save provider order?")?;
    Ok(())
}

fn parse_provider_order(raw: &str) -> Result<Vec<String>, String> {
    let mut providers = Vec::new();
    for part in raw.split(',') {
        let name = part.trim().to_lowercase();
        if name.is_empty() {
            continue;
        }
        if !KNOWN_PROVIDERS.contains(&name.as_str()) {
            return Err(format!(
                "Unknown provider \"{name}\". Use: {}.",
                KNOWN_PROVIDERS.join(", ")
            ));
        }
        if providers.iter().any(|existing| existing == &name) {
            return Err(format!("Duplicate provider \"{name}\"."));
        }
        providers.push(name);
    }
    if providers.is_empty() {
        return Err("At least one provider required.".to_string());
    }
    Ok(providers)
}

fn edit_musicbrainz(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(
        &["Settings", "Cover art", "MusicBrainz"],
        "MusicBrainz",
    )?;

    let Some(score_raw) = fields::prompt_text_with_help(
        "Minimum match score (0-100)",
        &config.cover.provider.musicbrainz.min_score.to_string(),
        "Higher = stricter matching.",
    )? else {
        return Ok(());
    };
    let score: u8 = score_raw.trim().parse().map_err(|_| {
        Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "score must be 0-100",
        ))
    })?;
    if score > 100 {
        return Err(Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "score must be 0-100",
        )));
    }

    let mut patch = ConfigPatch::default();
    patch.set_table_value(
        "cover.provider.musicbrainz",
        "min_score",
        toml_edit::Value::from(score as i64),
    );

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save MusicBrainz settings?")?;
    Ok(())
}

fn edit_catbox(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(&["Settings", "Cover art", "Catbox"], "Catbox")?;

    let Some(use_litter) = fields::prompt_bool_with_help(
        "Upload via temporary Litterbox?",
        config.cover.provider.catbox.use_litter,
        "When true, uploads expire automatically.",
    )? else {
        return Ok(());
    };

    let hours_default = config.cover.provider.catbox.litter_hours.to_string();
    let Some(hours_raw) = fields::prompt_text_with_help(
        "Litterbox hours (1, 12, 24, or 72)",
        &hours_default,
        "Expiry for Litterbox uploads.",
    )? else {
        return Ok(());
    };
    let hours: u8 = hours_raw.trim().parse().map_err(|_| {
        Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid litter hours",
        ))
    })?;
    let clamped = clamp_litter_hours(hours);

    let user_hash = fields::prompt_optional_text_with_help(
        "Catbox user hash (optional)",
        config.cover.provider.catbox.user_hash.as_deref(),
        "Optional hash for managing uploads on catbox.moe.",
    )?;

    let mut patch = ConfigPatch::default();
    patch.set_table_bool("cover.provider.catbox", "use_litter", use_litter);
    patch.set_table_value(
        "cover.provider.catbox",
        "litter_hours",
        toml_edit::Value::from(clamped as i64),
    );
    match user_hash {
        Some(hash) if !hash.trim().is_empty() => {
            patch.set_table_string("cover.provider.catbox", "user_hash", hash);
        }
        _ => {
            patch.remove_table_key("cover.provider.catbox", "user_hash");
        }
    }

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save Catbox settings?")?;
    Ok(())
}

fn edit_imgbb(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(&["Settings", "Cover art", "ImgBB"], "ImgBB")?;

    if config.cover.provider.imgbb.api_key.is_some() {
        ui::print_dim("API key: set (hidden)");
    } else {
        ui::print_dim("API key: not set");
    }

    let api_key = fields::prompt_optional_text_with_help(
        "ImgBB API key (optional)",
        None,
        "Obtain from https://api.imgbb.com/. Leave blank to keep current. Type '-' to clear.",
    )?;

    let Some(expiration_raw) = fields::prompt_text_with_help(
        "Expiration (seconds, 0 = never)",
        &config.cover.provider.imgbb.expiration.to_string(),
        "How long hosted images remain on ImgBB.",
    )? else {
        return Ok(());
    };
    let expiration: u64 = expiration_raw.trim().parse().map_err(|_| {
        Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid expiration",
        ))
    })?;

    let mut patch = ConfigPatch::default();
    match api_key {
        Some(key) if key.trim() == "-" => {
            patch.remove_table_key("cover.provider.imgbb", "api_key");
        }
        Some(key) if !key.trim().is_empty() => {
            patch.set_table_string("cover.provider.imgbb", "api_key", key);
        }
        _ => {}
    }
    patch.set_table_u64("cover.provider.imgbb", "expiration", expiration);

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save ImgBB settings?")?;
    Ok(())
}

fn reset_cover(config_path: &std::path::Path) -> Result<(), Error> {
    if !ui::confirm_save("Remove all user [cover] overrides?")? {
        return Ok(());
    }
    let mut edit = ConfigEdit::default();
    edit.remove_table("cover");
    apply_edit(config_path, &edit)?;
    Ok(())
}

fn clamp_litter_hours(hours: u8) -> u8 {
    if LITTER_HOURS.contains(&hours) {
        hours
    } else {
        24
    }
}

#[cfg(test)]
mod tests {
    use super::parse_provider_order;

    #[test]
    fn parse_provider_order_preserves_user_order() {
        assert_eq!(
            parse_provider_order("musicbrainz, catbox").unwrap(),
            vec!["musicbrainz".to_string(), "catbox".to_string()]
        );
    }

    #[test]
    fn parse_provider_order_normalizes_case() {
        assert_eq!(
            parse_provider_order("Catbox, MusicBrainz").unwrap(),
            vec!["catbox".to_string(), "musicbrainz".to_string()]
        );
    }

    #[test]
    fn parse_provider_order_rejects_unknown() {
        assert!(parse_provider_order("spotify, catbox").is_err());
    }

    #[test]
    fn parse_provider_order_rejects_duplicates() {
        assert!(parse_provider_order("catbox, catbox").is_err());
    }

    #[test]
    fn parse_provider_order_requires_at_least_one() {
        assert!(parse_provider_order("  ,  ").is_err());
    }
}
