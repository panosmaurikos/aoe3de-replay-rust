//! Importer for the aoe3-companion data set (MIT, Edgar Aparicio Baeza /
//! VitorRoda). Converts the companion's xml2json game files into our compact
//! canonical `data/*.json`, keyed by the game's `dbid` and with display names
//! resolved from the English string table.
//!
//! IMPORTANT: the `dbid` space here is the GAME data id space. It is NOT the
//! same as the card `rawId` space found in replay decks/commands — see
//! `docs/game-data-layer.md`. This importer builds a reference database; it does
//! not by itself resolve replay card ids.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ImportStats {
    pub cards: usize,
    pub techs: usize,
    pub units: usize,
    pub civs: usize,
    pub icons: usize,
    pub strings: usize,
}

/// Return `node[key]` as a list: a single object becomes a one-element list, an
/// array stays as-is, anything else (including missing) becomes empty.
fn children<'a>(node: &'a Value, key: &str) -> Vec<&'a Value> {
    match node.get(key) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    }
}

fn str_field<'a>(node: &'a Value, key: &str) -> Option<&'a str> {
    node.get(key).and_then(Value::as_str)
}

/// `flag` may be a string or an array of strings.
fn has_flag(node: &Value, flag: &str) -> bool {
    match node.get("flag") {
        Some(Value::String(value)) => value == flag,
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(flag)),
        _ => false,
    }
}

/// Derive (iconKey, normalizedPath) from a raw game icon path such as
/// `resources\images\icons\techs\native\Capitalism.png`.
fn icon_entry(prefix: &str, raw: &str) -> Option<(String, String)> {
    let normalized = raw.replace('\\', "/");
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    let stem = file.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file);
    if stem.is_empty() {
        return None;
    }
    Some((format!("{prefix}.{stem}"), normalized))
}

fn load_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("Could not read '{}': {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("Could not parse '{}': {err}", path.display()))
}

/// Resolve the companion `src/data` directory from a flexible input path.
fn resolve_data_dir(input: &Path) -> Result<PathBuf, String> {
    let candidates = [
        input.to_path_buf(),
        input.join("src").join("data"),
        input.join("data"),
    ];
    for candidate in candidates {
        if candidate.join("techtreey.xml.json").is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Could not find techtreey.xml.json under '{}' (expected the aoe3-companion repo or its src/data dir)",
        input.display()
    ))
}

/// Build a locid -> text map from the English string table.
fn load_strings(data_dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let value = load_json(&data_dir.join("localization").join("stringtabley_en.json"))?;
    let table = value
        .get("stringtable")
        .ok_or("stringtable key missing in stringtabley_en.json")?;
    let mut strings = BTreeMap::new();
    for language in children(table, "language") {
        for entry in children(language, "string") {
            if let (Some(locid), Some(text)) =
                (str_field(entry, "@_locid"), str_field(entry, "#text"))
            {
                strings.insert(locid.to_string(), text.to_string());
            }
        }
    }
    Ok(strings)
}

fn definition(
    dbid: &str,
    internal_name: Option<&str>,
    display_name: String,
    type_label: &str,
    icon_key: Option<&str>,
) -> Value {
    let mut entry = Map::new();
    if let Ok(id) = dbid.parse::<i64>() {
        entry.insert("id".to_string(), json!(id));
    } else {
        entry.insert("id".to_string(), json!(dbid));
    }
    if let Some(internal_name) = internal_name {
        entry.insert("internalName".to_string(), json!(internal_name));
    }
    entry.insert("displayName".to_string(), json!(display_name));
    entry.insert("type".to_string(), json!(type_label));
    if let Some(icon_key) = icon_key {
        entry.insert("iconKey".to_string(), json!(icon_key));
    }
    entry.insert("source".to_string(), json!("aoe3_companion"));
    entry.insert("confidence".to_string(), json!("imported"));
    Value::Object(entry)
}

fn write_pretty(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| format!("Could not serialize '{}': {err}", path.display()))?;
    fs::write(path, text).map_err(|err| format!("Could not write '{}': {err}", path.display()))
}

pub fn import_aoe3_companion(input: &Path, out: &Path) -> Result<ImportStats, String> {
    let data_dir = resolve_data_dir(input)?;
    let strings = load_strings(&data_dir)?;
    let resolve = |locid: Option<&str>, fallback: &str| -> String {
        locid
            .and_then(|id| strings.get(id).cloned())
            .unwrap_or_else(|| fallback.to_string())
    };

    fs::create_dir_all(out)
        .map_err(|err| format!("Could not create out dir '{}': {err}", out.display()))?;

    let mut cards = Map::new();
    let mut techs = Map::new();
    let mut units = Map::new();
    let mut civs = Map::new();
    let mut icons: BTreeMap<String, Value> = BTreeMap::new();

    let mut add_icon = |prefix: &str, raw: Option<&str>| -> Option<String> {
        let (key, path) = icon_entry(prefix, raw?)?;
        icons.entry(key.clone()).or_insert_with(|| {
            json!({ "path": path, "source": "aoe3_companion", "fallback": false })
        });
        Some(key)
    };

    // Techs and home-city cards.
    let techtree = load_json(&data_dir.join("techtreey.xml.json"))?;
    let techtree = techtree
        .get("techtree")
        .ok_or("techtree key missing in techtreey.xml.json")?;
    for tech in children(techtree, "tech") {
        let Some(dbid) = str_field(tech, "dbid") else {
            continue;
        };
        let name = str_field(tech, "@name");
        let display = resolve(
            str_field(tech, "displaynameid"),
            name.unwrap_or("Unknown Tech"),
        );
        let is_card = has_flag(tech, "HomeCity");
        let prefix = if is_card { "card" } else { "tech" };
        let icon_key = add_icon(prefix, str_field(tech, "icon"));
        let type_label = if is_card { "home_city_card" } else { "tech" };
        let entry = definition(dbid, name, display, type_label, icon_key.as_deref());
        if is_card {
            cards.insert(dbid.to_string(), entry);
        } else {
            techs.insert(dbid.to_string(), entry);
        }
    }

    // Proto units.
    let proto = load_json(&data_dir.join("protoy.xml.json"))?;
    let proto = proto
        .get("proto")
        .ok_or("proto key missing in protoy.xml.json")?;
    for unit in children(proto, "unit") {
        let Some(dbid) = str_field(unit, "dbid") else {
            continue;
        };
        let name = str_field(unit, "@name");
        let display = resolve(
            str_field(unit, "displaynameid"),
            name.unwrap_or("Unknown Unit"),
        );
        let icon_key = add_icon("unit", str_field(unit, "icon"));
        units.insert(
            dbid.to_string(),
            definition(dbid, name, display, "unit", icon_key.as_deref()),
        );
    }

    // Civilizations (keyed by internal civ name; civs have no dbid).
    let civ_root = load_json(&data_dir.join("civs.xml.json"))?;
    if let Some(civ_root) = civ_root.get("civs") {
        for civ in children(civ_root, "civ") {
            let Some(name) = str_field(civ, "name") else {
                continue;
            };
            let display = resolve(str_field(civ, "displaynameid"), name);
            let icon_key = add_icon("civ", str_field(civ, "portrait"));
            let mut entry = Map::new();
            entry.insert("internalName".to_string(), json!(name));
            entry.insert("displayName".to_string(), json!(display));
            if let Some(icon_key) = icon_key {
                entry.insert("iconKey".to_string(), json!(icon_key));
            }
            entry.insert("source".to_string(), json!("aoe3_companion"));
            entry.insert("confidence".to_string(), json!("imported"));
            civs.insert(name.to_string(), Value::Object(entry));
        }
    }

    let stats = ImportStats {
        cards: cards.len(),
        techs: techs.len(),
        units: units.len(),
        civs: civs.len(),
        icons: icons.len(),
        strings: strings.len(),
    };

    let icons_value = Value::Object(icons.into_iter().collect());
    write_pretty(&out.join("cards.json"), &Value::Object(cards))?;
    write_pretty(&out.join("techs.json"), &Value::Object(techs))?;
    write_pretty(&out.join("units.json"), &Value::Object(units))?;
    write_pretty(&out.join("civs.json"), &Value::Object(civs))?;
    write_pretty(&out.join("icons.json"), &icons_value)?;

    Ok(stats)
}
