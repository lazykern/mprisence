use inquire::Select;
use inquire::InquireError;

use crate::config::{self, schema::ActivityType};
use crate::error::Error;
use crate::setup::fields;
use crate::setup::global::save_global_edit;
use crate::setup::patch::{apply_edit, ConfigEdit, ConfigPatch};
use crate::setup::ui::{self, prompt_select, BACK_LABEL};

const TEMPLATE_HELP: &str =
    "Handlebars template. See config.example.toml for variables and helpers.";

pub fn run(config_path: &std::path::Path) -> Result<(), Error> {
    loop {
        let (_, config) = config::load_merged_config(Some(config_path))?;
        ui::redraw(&["Settings", "Display"], "Display")?;

        ui::print_dim(&format!(
            "Activity: {:?} (content-type: {})",
            config.activity_type.default,
            if config.activity_type.use_content_type {
                "on"
            } else {
                "off"
            }
        ));
        ui::print_dim(&format!(
            "Time: show={}, as_elapsed={}",
            config.time.show, config.time.as_elapsed
        ));
        ui::print_dim(&format!("Template details: {}", config.template.details));
        println!();

        let choice = match prompt_select(
            "",
            vec![
                "Activity and time".to_string(),
                "Templates".to_string(),
                "Reset display to bundled defaults".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Activity and time" => edit_activity_time(config_path, &config)?,
            "Templates" => edit_templates(config_path, &config)?,
            "Reset display to bundled defaults" => reset_display(config_path)?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn edit_activity_time(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(&["Settings", "Display", "Activity & time"], "Activity & time")?;

    let Some(use_content_type) = fields::prompt_bool_with_help(
        "Detect activity type from media content?",
        config.activity_type.use_content_type,
        "audio -> listening, video -> watching, etc.",
    )? else {
        return Ok(());
    };

    let Some(default) = prompt_default_activity_type(config.activity_type.default)? else {
        return Ok(());
    };

    let Some(show_time) = fields::prompt_bool_with_help(
        "Show playback time in Discord?",
        config.time.show,
        "Progress bar for listening; elapsed/remaining for other types.",
    )? else {
        return Ok(());
    };

    let Some(as_elapsed) = fields::prompt_bool_with_help(
        "Show elapsed time (vs remaining)?",
        config.time.as_elapsed,
        "true = 1:23 elapsed, false = -1:23 remaining.",
    )? else {
        return Ok(());
    };

    let mut patch = ConfigPatch::default();
    patch.set_table_bool("activity_type", "use_content_type", use_content_type);
    patch.set_table_string(
        "activity_type",
        "default",
        activity_type_to_str(default),
    );
    patch.set_table_bool("time", "show", show_time);
    patch.set_table_bool("time", "as_elapsed", as_elapsed);

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save activity and time settings?")?;
    Ok(())
}

fn edit_templates(
    config_path: &std::path::Path,
    config: &config::schema::Config,
) -> Result<(), Error> {
    ui::redraw(&["Settings", "Display", "Templates"], "Templates")?;

    let Some(details) = fields::prompt_text_with_help(
        "Template details (first line)",
        &config.template.details,
        TEMPLATE_HELP,
    )? else {
        return Ok(());
    };
    let Some(state) = fields::prompt_text_with_help(
        "Template state (second line)",
        &config.template.state,
        TEMPLATE_HELP,
    )? else {
        return Ok(());
    };
    let Some(large_text) = fields::prompt_text_with_help(
        "Template large_text (hover on cover art)",
        &config.template.large_text,
        TEMPLATE_HELP,
    )? else {
        return Ok(());
    };
    let Some(small_text) = fields::prompt_text_with_help(
        "Template small_text (hover on player icon)",
        &config.template.small_text,
        TEMPLATE_HELP,
    )? else {
        return Ok(());
    };

    let mut patch = ConfigPatch::default();
    patch.set_table_string("template", "details", details);
    patch.set_table_string("template", "state", state);
    patch.set_table_string("template", "large_text", large_text);
    patch.set_table_string("template", "small_text", small_text);

    let edit = ConfigEdit {
        patch,
        ..Default::default()
    };
    let _ = save_global_edit(config_path, &edit, "Save template settings?")?;
    Ok(())
}

fn reset_display(config_path: &std::path::Path) -> Result<(), Error> {
    if !ui::confirm_save(
        "Remove user [activity_type], [time], and [template] overrides?",
    )? {
        return Ok(());
    }
    let mut edit = ConfigEdit::default();
    edit.remove_table("activity_type");
    edit.remove_table("time");
    edit.remove_table("template");
    apply_edit(config_path, &edit)?;
    Ok(())
}

fn prompt_default_activity_type(default: ActivityType) -> Result<Option<ActivityType>, Error> {
    let options = vec![
        ("listening", ActivityType::Listening),
        ("watching", ActivityType::Watching),
        ("playing", ActivityType::Playing),
        ("competing", ActivityType::Competing),
    ];
    let labels: Vec<String> = options.iter().map(|(label, _)| label.to_string()).collect();
    let default_idx = options
        .iter()
        .position(|(_, value)| *value == default)
        .unwrap_or(0);

    let choice = match Select::new("Default activity type", labels)
        .with_starting_cursor(default_idx)
        .prompt()
    {
        Ok(value) => value,
        Err(InquireError::OperationCanceled) => return Ok(None),
        Err(err) => {
            return Err(Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )))
        }
    };

    Ok(Some(
        options
            .into_iter()
            .find(|(label, _)| *label == choice)
            .map(|(_, value)| value)
            .ok_or_else(|| {
                Error::IO(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "missing activity type option",
                ))
            })?,
    ))
}

fn activity_type_to_str(value: ActivityType) -> &'static str {
    match value {
        ActivityType::Listening => "listening",
        ActivityType::Watching => "watching",
        ActivityType::Playing => "playing",
        ActivityType::Competing => "competing",
    }
}
