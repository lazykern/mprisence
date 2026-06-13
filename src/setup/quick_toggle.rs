use std::collections::HashSet;
use std::path::Path;

use inquire::MultiSelect;

use crate::error::Error;
use crate::setup::patch::{apply_edit, ConfigEdit, ConfigPatch};
use crate::setup::ui::{self, confirm_save};

#[derive(Clone)]
pub struct ToggleEntry {
    pub key: String,
    pub label: String,
    pub enabled: bool,
}

pub fn run_quick_toggle(
    breadcrumb: &[&str],
    table_section: &str,
    entity_label: &str,
    entries: Vec<ToggleEntry>,
    config_path: &Path,
) -> Result<(), Error> {
    let mut crumbs: Vec<&str> = breadcrumb.to_vec();
    crumbs.push("Quick toggle");
    ui::redraw(&crumbs, "Show in Discord")?;
    ui::print_dim("Checked = active (ignore = false). Unchecked = hidden (ignore = true).");

    if entries.is_empty() {
        ui::print_dim(&format!("No {entity_label}s found."));
        return Ok(());
    }

    let defaults: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, opt)| opt.enabled)
        .map(|(idx, _)| idx)
        .collect();

    let labels: Vec<String> = entries.iter().map(|opt| opt.label.clone()).collect();

    let selected = match MultiSelect::new("", labels.clone())
        .with_default(&defaults)
        .prompt()
    {
        Ok(values) => values,
        Err(inquire::InquireError::OperationCanceled) => return Ok(()),
        Err(err) => {
            return Err(Error::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )))
        }
    };

    let selected_labels: HashSet<&str> = selected.iter().map(String::as_str).collect();
    let mut patch = ConfigPatch::default();
    let mut changes = 0usize;

    for opt in &entries {
        let want_enabled = selected_labels.contains(opt.label.as_str());
        if want_enabled == opt.enabled {
            continue;
        }
        let table = format!("{table_section}.{}", opt.key);
        patch.set_table_bool(&table, "ignore", !want_enabled);
        changes += 1;
    }

    if changes == 0 {
        ui::print_dim(&format!("No {entity_label} changes."));
        return Ok(());
    }

    ui::redraw(&crumbs, "Save changes")?;
    if !confirm_save(&format!("Save {changes} {entity_label} override(s)?"))? {
        return Ok(());
    }

    apply_edit(config_path, &ConfigEdit { patch, ..Default::default() })?;
    ui::print_dim(&format!("Saved to {}", config_path.display()));
    Ok(())
}
