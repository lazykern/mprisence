use std::collections::BTreeMap;
use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::config;
use crate::error::Error;

#[derive(Debug, Default, Clone)]
pub struct TablePatch {
    pub entries: BTreeMap<String, Value>,
    pub removed_keys: Vec<String>,
}

/// User-config overrides to apply.
#[derive(Debug, Default, Clone)]
pub struct ConfigPatch {
    pub top_level: BTreeMap<String, Value>,
    pub tables: BTreeMap<String, TablePatch>,
}

/// Patch + table removals for setup edits.
#[derive(Debug, Default, Clone)]
pub struct ConfigEdit {
    pub patch: ConfigPatch,
    pub removed_tables: Vec<String>,
    pub removed_top_level: Vec<String>,
}

impl TablePatch {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.removed_keys.is_empty()
    }

    pub fn remove_key(&mut self, key: &str) {
        self.removed_keys.push(key.to_string());
    }
}

impl ConfigPatch {
    pub fn set_table_bool(&mut self, table: &str, key: &str, value: bool) {
        self.set_table_value(table, key, Value::from(value));
    }

    pub fn set_table_string(&mut self, table: &str, key: &str, value: impl Into<String>) {
        self.set_table_value(table, key, Value::from(value.into()));
    }

    pub fn set_table_u64(&mut self, table: &str, key: &str, value: u64) {
        self.set_table_value(table, key, Value::from(value as i64));
    }

    pub fn set_table_value(&mut self, table: &str, key: &str, value: Value) {
        self.tables
            .entry(table.to_string())
            .or_default()
            .entries
            .insert(key.to_string(), value);
    }

    pub fn set_table_array(&mut self, table: &str, key: &str, values: &[impl AsRef<str>]) {
        let mut array = Array::new();
        for value in values {
            array.push(value.as_ref());
        }
        self.set_table_value(table, key, Value::Array(array));
    }

    pub fn remove_table_key(&mut self, table: &str, key: &str) {
        self.tables
            .entry(table.to_string())
            .or_default()
            .remove_key(key);
    }

    pub fn set_top_level_bool(&mut self, key: &str, value: bool) {
        self.top_level.insert(key.to_string(), Value::from(value));
    }

    pub fn set_top_level_u64(&mut self, key: &str, value: u64) {
        self.top_level
            .insert(key.to_string(), Value::from(value as i64));
    }

    pub fn set_top_level_string_array(&mut self, key: &str, values: &[impl AsRef<str>]) {
        let mut array = Array::new();
        for value in values {
            array.push(value.as_ref());
        }
        self.top_level.insert(key.to_string(), Value::Array(array));
    }

    pub fn is_empty(&self) -> bool {
        self.top_level.is_empty() && self.tables.is_empty()
    }
}

impl ConfigEdit {
    pub fn remove_table(&mut self, table_path: &str) {
        self.removed_tables.push(table_path.to_string());
    }

    pub fn remove_top_level(&mut self, key: &str) {
        self.removed_top_level.push(key.to_string());
    }

    pub fn is_empty(&self) -> bool {
        self.removed_tables.is_empty()
            && self.removed_top_level.is_empty()
            && self.patch.is_empty()
    }
}

pub fn ensure_config_file(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        let header = "# mprisence user config\n# Edited via `mprisence setup`\n\n";
        std::fs::write(path, header)?;
    }
    Ok(())
}

pub fn load_document(path: &Path) -> Result<DocumentMut, Error> {
    ensure_config_file(path)?;
    let contents = std::fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    contents.parse::<DocumentMut>().map_err(|err| {
        Error::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid TOML in {}: {err}", path.display()),
        ))
    })
}

pub fn apply_edit(path: &Path, edit: &ConfigEdit) -> Result<(), Error> {
    if edit.is_empty() {
        return Ok(());
    }

    let mut doc = load_document(path)?;

    for table_path in &edit.removed_tables {
        remove_nested_table(&mut doc, table_path);
    }

    for key in &edit.removed_top_level {
        doc.remove(key);
    }

    for (key, value) in &edit.patch.top_level {
        doc[key] = item_from_value(value);
    }

    for (table_path, table_patch) in &edit.patch.tables {
        let Some(table) = get_or_create_table(&mut doc, table_path, true) else {
            continue;
        };

        for key in &table_patch.removed_keys {
            table.remove(key);
        }

        for (key, value) in &table_patch.entries {
            table[key] = item_from_value(value);
        }

        prune_empty_tables(&mut doc, table_path);
    }

    std::fs::write(path, doc.to_string())?;
    config::validate_config_file(Some(path))?;
    Ok(())
}

fn get_or_create_table<'a>(
    doc: &'a mut DocumentMut,
    table_path: &str,
    create: bool,
) -> Option<&'a mut Table> {
    let segments: Vec<&str> = table_path.split('.').collect();
    if segments.is_empty() {
        return None;
    }

    if segments.len() == 1 {
        let key = segments[0];
        if create {
            let item = doc.entry(key).or_insert(Item::Table(Table::new()));
            if !item.is_table() {
                *item = Item::Table(Table::new());
            }
            return item.as_table_mut();
        }
        return doc.get_mut(key).and_then(|item| item.as_table_mut());
    }

    let mut current: &mut Table = {
        let root = segments[0];
        if create {
            let item = doc.entry(root).or_insert(Item::Table(Table::new()));
            if !item.is_table() {
                *item = Item::Table(Table::new());
            }
            item.as_table_mut().expect("table just ensured")
        } else {
            doc.get_mut(root)?.as_table_mut()?
        }
    };

    for segment in &segments[1..segments.len() - 1] {
        let item = if create {
            current
                .entry(segment)
                .or_insert(Item::Table(Table::new()))
        } else {
            current.get_mut(segment)?
        };
        if create && !item.is_table() {
            *item = Item::Table(Table::new());
        }
        current = item.as_table_mut()?;
    }

    let leaf = segments[segments.len() - 1];
    let item = if create {
        current
            .entry(leaf)
            .or_insert(Item::Table(Table::new()))
    } else {
        current.get_mut(leaf)?
    };
    if create && !item.is_table() {
        *item = Item::Table(Table::new());
    }
    item.as_table_mut()
}

fn prune_empty_tables(doc: &mut DocumentMut, table_path: &str) {
    let segments: Vec<&str> = table_path.split('.').collect();
    if segments.is_empty() {
        return;
    }

    for len in (1..=segments.len()).rev() {
        let segs = &segments[..len];
        if segs.len() == 1 {
            if let Some(item) = doc.get(segs[0]) {
                if let Some(table) = item.as_table() {
                    if table.is_empty() {
                        doc.remove(segs[0]);
                    }
                }
            }
            continue;
        }

        let mut parent_opt: Option<&mut Table> =
            doc.get_mut(segs[0]).and_then(|i| i.as_table_mut());
        for segment in &segs[1..segs.len() - 1] {
            parent_opt = parent_opt.and_then(|t| t.get_mut(segment).and_then(|i| i.as_table_mut()));
        }
        let leaf = segs[segs.len() - 1];
        if let Some(parent) = parent_opt {
            if let Some(item) = parent.get(leaf) {
                if let Some(table) = item.as_table() {
                    if table.is_empty() {
                        parent.remove(leaf);
                    }
                }
            }
            if parent.is_empty() && segs.len() == 2 {
                doc.remove(segs[0]);
            }
        }
    }
}

fn remove_nested_table(doc: &mut DocumentMut, table_path: &str) {
    let segments: Vec<&str> = table_path.split('.').collect();
    if segments.is_empty() {
        return;
    }

    if segments.len() == 1 {
        doc.remove(segments[0]);
        return;
    }

    let mut current = match doc.get_mut(segments[0]).and_then(|i| i.as_table_mut()) {
        Some(table) => table,
        None => return,
    };

    for segment in &segments[1..segments.len() - 1] {
        current = match current.get_mut(segment).and_then(|i| i.as_table_mut()) {
            Some(table) => table,
            None => return,
        };
    }

    let leaf = segments[segments.len() - 1];
    current.remove(leaf);

    // Prune empty ancestors
    prune_empty_tables(doc, table_path);
}

fn item_from_value(value: &Value) -> Item {
    Item::Value(value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn apply_patch(path: &Path, patch: &ConfigPatch) -> Result<(), Error> {
        apply_edit(
            path,
            &ConfigEdit {
                patch: patch.clone(),
                ..Default::default()
            },
        )
    }

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mprisence-setup-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn patch_preserves_existing_comments() {
        let temp_dir = temp_path("comments");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");
        fs::write(
            &path,
            "# keep this comment\ninterval = 3000\n\n[player.spotify]\nignore = true\n",
        )
        .expect("write");

        let mut patch = ConfigPatch::default();
        patch.set_table_bool("player.spotify", "ignore", false);

        apply_patch(&path, &patch).expect("patch");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(updated.contains("# keep this comment"));
        assert!(updated.contains("interval = 3000"));
        assert!(updated.contains("ignore = false"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn patch_creates_new_table() {
        let temp_dir = temp_path("new");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");

        let mut patch = ConfigPatch::default();
        patch.set_table_bool("web_player.youtube_music", "ignore", false);

        apply_patch(&path, &patch).expect("patch");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(updated.contains("[web_player.youtube_music]"));
        assert!(updated.contains("ignore = false"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn patch_single_level_table() {
        let temp_dir = temp_path("time");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");

        let mut patch = ConfigPatch::default();
        patch.set_table_bool("time", "show", false);

        apply_patch(&path, &patch).expect("patch");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(updated.contains("[time]"));
        assert!(updated.contains("show = false"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn patch_deep_nested_table() {
        let temp_dir = temp_path("imgbb");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");

        let mut patch = ConfigPatch::default();
        patch.set_table_string("cover.provider.imgbb", "api_key", "secret-key");

        apply_patch(&path, &patch).expect("patch");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(updated.contains("[cover.provider.imgbb]"));
        assert!(updated.contains("api_key = \"secret-key\""));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn edit_removes_user_table() {
        let temp_dir = temp_path("rm");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");
        fs::write(
            &path,
            "[player.spotify]\nignore = false\napp_id = \"123\"\n\n[player.custom]\nignore = false\n",
        )
        .expect("write");

        let mut edit = ConfigEdit::default();
        edit.remove_table("player.spotify");

        apply_edit(&path, &edit).expect("edit");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(!updated.contains("[player.spotify]"));
        assert!(updated.contains("[player.custom]"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn remove_table_key_prunes_empty_parents() {
        let temp_dir = temp_path("prune");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");
        fs::write(
            &path,
            "[cover.provider.imgbb]\napi_key = \"x\"\n",
        )
        .expect("write");

        let mut patch = ConfigPatch::default();
        patch.remove_table_key("cover.provider.imgbb", "api_key");

        apply_patch(&path, &patch).expect("patch");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(!updated.contains("api_key"));
        assert!(!updated.contains("[cover.provider.imgbb]"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn patch_top_level_string_array() {
        let temp_dir = temp_path("allowed");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");

        let mut patch = ConfigPatch::default();
        patch.set_top_level_string_array("allowed_players", &["vlc", "*mpd*"]);

        apply_patch(&path, &patch).expect("patch");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(updated.contains("allowed_players = [\"vlc\", \"*mpd*\"]"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn edit_removes_top_level_key() {
        let temp_dir = temp_path("top-rm");
        fs::create_dir_all(&temp_dir).expect("temp dir");
        let path = temp_dir.join("config.toml");
        fs::write(&path, "allowed_players = [\"vlc\"]\ninterval = 3000\n").expect("write");

        let mut edit = ConfigEdit::default();
        edit.remove_top_level("allowed_players");

        apply_edit(&path, &edit).expect("edit");

        let updated = fs::read_to_string(&path).expect("read");
        assert!(!updated.contains("allowed_players"));
        assert!(updated.contains("interval = 3000"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
