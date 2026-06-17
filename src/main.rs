use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use aoe3de_replay_rust::gamedata::{CardDefinition, GameData, NamedRef};
use aoe3de_replay_rust::{parse_all_with_options, ParseOptions};
use serde_json::{json, Value};

mod import;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse(env::args().skip(1).collect())?;

    match cli.command {
        Command::Parse {
            replay_path,
            output_path,
            debug_commands,
            experimental_shipments,
            events,
        } => {
            let file_bytes = fs::read(&replay_path).map_err(|err| {
                format!(
                    "Could not read replay file '{}': {err}",
                    replay_path.display()
                )
            })?;

            let parsed = parse_all_with_options(
                &file_bytes,
                ParseOptions {
                    debug_commands,
                    experimental_shipments,
                    events,
                },
            )?;
            let json = serde_json::to_string_pretty(&parsed)
                .map_err(|err| format!("Could not serialize parsed replay to JSON: {err}"))?;
            write_json(&output_path, json)?;
            println!("Saved JSON to: {}", output_path.display());
        }
        Command::Normalize {
            parsed_path,
            output_path,
        } => {
            let parsed_json = read_json_file(&parsed_path)?;
            let normalized = normalize_parsed_json(parsed_json)?;
            let json = serde_json::to_string_pretty(&normalized)
                .map_err(|err| format!("Could not serialize normalized JSON: {err}"))?;
            write_json(&output_path, json)?;
            println!("Saved JSON to: {}", output_path.display());
        }
        Command::Validate { parsed_path } => {
            let parsed_json = read_json_file(&parsed_path)?;
            let report = validate_parsed_json(&parsed_json);
            report.print();
            if report.has_errors() {
                return Err(format!(
                    "Validation failed: {} error(s), {} warning(s)",
                    report.errors.len(),
                    report.warnings.len()
                ));
            }
            println!(
                "Validation passed with {} warning(s)",
                report.warnings.len()
            );
        }
        Command::InspectCommands {
            parsed_path,
            options,
        } => {
            let parsed_json = read_json_file(&parsed_path)?;
            inspect_commands(&parsed_json, &options)?;
        }
        Command::InspectCardCommands {
            parsed_path,
            actor_slot,
        } => {
            let parsed_json = read_json_file(&parsed_path)?;
            inspect_card_commands(&parsed_json, actor_slot)?;
        }
        Command::CompareCommands { options } => {
            let a_json = read_json_file(&options.a_path)?;
            let b_json = read_json_file(&options.b_path)?;
            compare_commands(&a_json, &b_json, &options)?;
        }
        Command::CompareSummaries { a_path, b_path } => {
            let a_json = read_json_file(&a_path)?;
            let b_json = read_json_file(&b_path)?;
            compare_summaries(&a_path, &a_json, &b_path, &b_json)?;
        }
        Command::DumpDecks {
            parsed_path,
            options,
        } => {
            let parsed_json = read_json_file(&parsed_path)?;
            dump_decks(&parsed_json, &options)?;
        }
        Command::PlayerSummary { parsed_path } => {
            let parsed_json = read_json_file(&parsed_path)?;
            player_summary(&parsed_json)?;
        }
        Command::ResolveCard { card_id } => {
            resolve_card(card_id);
        }
        Command::ResolveUnit { unit_id } => {
            let game_data = GameData::embedded();
            let unit = game_data.resolve_unit(unit_id);
            print_named_ref("Unit", unit_id, &unit, game_data.unit(unit_id));
        }
        Command::ResolveTech { tech_id } => {
            let game_data = GameData::embedded();
            let tech = game_data.resolve_tech(tech_id);
            print_named_ref("Tech", tech_id, &tech, game_data.card(tech_id));
        }
        Command::ResolveBuilding { building_id } => {
            let game_data = GameData::embedded();
            let building = game_data.resolve_building(building_id);
            print_named_ref("Building", building_id, &building, game_data.unit(building_id));
        }
        Command::ValidateCorpus { dir } => {
            validate_corpus(&dir)?;
        }
        Command::ImportAoe3Companion { input, out } => {
            let stats = import::import_aoe3_companion(&input, &out)?;
            println!("Imported aoe3-companion data into {}", out.display());
            println!(
                "  cards={} techs={} units={} civs={} icons={} (from {} strings)",
                stats.cards, stats.techs, stats.units, stats.civs, stats.icons, stats.strings
            );
        }
        Command::Capture {
            offsets_path,
            hz,
            duration_s,
            output_path,
        } => {
            run_capture(&offsets_path, hz, duration_s, output_path.as_deref())?;
        }
        Command::MergeCapture {
            replay_path,
            capture_path,
            offset_ms,
            output_path,
        } => {
            run_merge_capture(&replay_path, &capture_path, offset_ms, output_path.as_deref())?;
        }
    }

    Ok(())
}

/// Mode B: attach a live capture's per-player series to a parsed replay JSON
/// under a `liveState` key (aligned by `offset_ms`). See docs/mode-b-live-capture.md.
fn run_merge_capture(
    replay_path: &Path,
    capture_path: &Path,
    offset_ms: i64,
    output_path: Option<&Path>,
) -> Result<(), String> {
    use aoe3de_replay_rust::mode_b::LiveCapture;

    let mut replay_json = read_json_file(replay_path)?;
    let capture_text = fs::read_to_string(capture_path)
        .map_err(|e| format!("could not read capture '{}': {e}", capture_path.display()))?;
    let capture: LiveCapture = serde_json::from_str(&capture_text)
        .map_err(|e| format!("invalid capture JSON '{}': {e}", capture_path.display()))?;

    let live_state = capture.to_live_state(offset_ms);
    let live_value = serde_json::to_value(&live_state)
        .map_err(|e| format!("could not serialize liveState: {e}"))?;
    match replay_json {
        Value::Object(ref mut map) => {
            map.insert("liveState".to_string(), live_value);
        }
        _ => return Err("replay JSON is not an object — pass a parsed/normalized replay".into()),
    }

    let json = serde_json::to_string_pretty(&replay_json)
        .map_err(|e| format!("could not serialize merged JSON: {e}"))?;
    match output_path {
        Some(path) => {
            write_json(path, json)?;
            println!(
                "Merged {} player series into {}",
                live_state.players.len(),
                path.display()
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

/// Mode B: attach to the running game and sample live state into a JSON capture.
/// See `docs/mode-b-live-capture.md`.
fn run_capture(
    offsets_path: &Path,
    hz: u32,
    duration_s: u64,
    output_path: Option<&Path>,
) -> Result<(), String> {
    use aoe3de_replay_rust::mode_b::{CaptureConfig, LiveCapture, PlatformProcess, Sampler};
    use std::time::{Duration, Instant};

    if hz == 0 {
        return Err("--hz must be >= 1".into());
    }

    let cfg = CaptureConfig::load(offsets_path)?;
    eprintln!(
        "Capture: config '{}' (game {}), process {}, {} resources @ {hz} Hz for {duration_s}s",
        offsets_path.display(),
        cfg.game_version,
        cfg.process_name,
        cfg.resources.len()
    );

    let mem = PlatformProcess::attach(&cfg)?;
    let sampler = Sampler::resolve(&cfg, &mem)?;
    sampler.check_sanity()?;
    eprintln!("Attached, game instance resolved, sanity check passed. Sampling... (Ctrl-C to stop early)");

    let interval = Duration::from_millis(1000 / hz as u64);
    let start = Instant::now();
    let deadline = start + Duration::from_secs(duration_s);
    let mut samples = Vec::new();
    let mut next = start;
    while Instant::now() < deadline {
        let t_ms = start.elapsed().as_millis() as u64;
        match sampler.sample(t_ms) {
            Ok(s) => samples.push(s),
            Err(e) => {
                eprintln!("WARN sample at {t_ms}ms failed: {e}");
            }
        }
        next += interval;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
    }

    let capture = LiveCapture {
        source: sampler.source_tag(),
        game_version: cfg.game_version.clone(),
        sample_hz: hz,
        samples,
    };
    let json = serde_json::to_string_pretty(&capture)
        .map_err(|err| format!("Could not serialize capture: {err}"))?;
    match output_path {
        Some(path) => {
            write_json(path, json)?;
            println!(
                "Captured {} samples to {}",
                capture.samples.len(),
                path.display()
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

fn resolve_card(card_id: i32) {
    let game_data = GameData::embedded();
    let card = game_data.resolve_card(card_id);
    let definition = game_data.card(card_id);

    println!("Card {card_id}");
    println!("Name: {}", card.display_name);
    println!(
        "Internal: {}",
        definition
            .and_then(|card| card.internal_name.as_deref())
            .unwrap_or("(unknown)")
    );
    println!("Icon: {}", card.icon_key);
    if let Some(icon) = game_data.icon(&card.icon_key) {
        println!("Icon path: {}", icon.path);
    } else {
        println!("Icon path: (generic fallback)");
    }
    println!(
        "Source: {}",
        definition
            .and_then(|card| card.source.as_deref())
            .unwrap_or("(none)")
    );
    println!(
        "Confidence: {}",
        definition
            .and_then(|card| card.confidence.as_deref())
            .unwrap_or("(none)")
    );
    if !card.known {
        println!("(card id has no entry in data/cards.json)");
    }
}

fn print_named_ref(kind: &str, id: i32, named: &NamedRef, definition: Option<&CardDefinition>) {
    println!("{kind} {id}");
    println!("Name: {}", named.display_name);
    println!(
        "Internal: {}",
        definition
            .and_then(|def| def.internal_name.as_deref())
            .unwrap_or("(unknown)")
    );
    if let Some(dbid) = definition.and_then(|def| def.dbid) {
        println!("Dbid: {dbid}");
    }
    println!("Icon: {}", named.icon_key);
    if !named.known {
        println!("({} id has no entry in the game data)", kind.to_lowercase());
    }
}

fn collect_replays(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_replays(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("age3Yrec") {
            out.push(path);
        }
    }
}

/// Parse + validate every `.age3Yrec` under a directory and report a QA summary
/// (parsed/failed, warnings, decode coverage). Robustness check for a replay
/// corpus — no panic on a bad file.
fn validate_corpus(dir: &Path) -> Result<(), String> {
    let mut replays = Vec::new();
    collect_replays(dir, &mut replays);
    replays.sort();
    if replays.is_empty() {
        return Err(format!("No .age3Yrec files found under '{}'", dir.display()));
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut total_warnings = 0usize;
    let mut unknown_ids: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    println!("Validating {} replay(s) under {}\n", replays.len(), dir.display());
    for path in &replays {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                println!("FAIL  {name}  (read error: {err})");
                failed += 1;
                continue;
            }
        };
        let options = ParseOptions {
            debug_commands: true,
            experimental_shipments: false,
            events: true,
        };
        let parsed = match parse_all_with_options(&bytes, options) {
            Ok(parsed) => parsed,
            Err(err) => {
                println!("FAIL  {name}  (parse error: {err})");
                failed += 1;
                continue;
            }
        };
        let value = serde_json::to_value(&parsed)
            .map_err(|err| format!("Could not serialize '{name}': {err}"))?;
        let report = validate_parsed_json(&value);

        let events = value
            .get("timeline")
            .and_then(|t| t.get("events"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let (cmd_total, unknown_total) = corpus_coverage(&value, &mut unknown_ids);
        let coverage = if cmd_total > 0 {
            100.0 * (cmd_total - unknown_total) as f64 / cmd_total as f64
        } else {
            100.0
        };

        total_warnings += report.warnings.len();
        if report.has_errors() {
            failed += 1;
            println!(
                "FAIL  {name}  ({} error(s), {} warning(s))",
                report.errors.len(),
                report.warnings.len()
            );
            for error in &report.errors {
                println!("        - {error}");
            }
        } else {
            passed += 1;
            println!(
                "OK    {name}  events={events} coverage={coverage:.1}% warnings={}",
                report.warnings.len()
            );
        }
    }

    println!("\nSummary: {passed} passed, {failed} failed, {total_warnings} warning(s) total");
    if !unknown_ids.is_empty() {
        let mut ids = unknown_ids.into_iter().collect::<Vec<_>>();
        ids.sort_by(|a, b| b.1.cmp(&a.1));
        let top = ids
            .iter()
            .take(8)
            .map(|(id, count)| format!("{id}:{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("Unclassified commandIds across corpus (id:count): {top}");
    }

    if failed > 0 {
        return Err(format!("{failed} replay(s) failed validation"));
    }
    Ok(())
}

fn corpus_coverage(
    value: &Value,
    unknown_ids: &mut std::collections::BTreeMap<String, usize>,
) -> (usize, usize) {
    let summary = value
        .get("debug")
        .and_then(|debug| debug.get("debugSummary"));
    let sum_map = |key: &str| -> usize {
        summary
            .and_then(|s| s.get(key))
            .and_then(Value::as_object)
            .map(|map| map.values().filter_map(Value::as_u64).map(|v| v as usize).sum())
            .unwrap_or(0)
    };
    if let Some(unknown) = summary
        .and_then(|s| s.get("unknownCommandIds"))
        .and_then(Value::as_object)
    {
        for (id, count) in unknown {
            if let Some(count) = count.as_u64() {
                *unknown_ids.entry(id.clone()).or_insert(0) += count as usize;
            }
        }
    }
    (sum_map("commandIds"), sum_map("unknownCommandIds"))
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let json_text = fs::read_to_string(path)
        .map_err(|err| format!("Could not read JSON file '{}': {err}", path.display()))?;
    serde_json::from_str(&json_text)
        .map_err(|err| format!("Could not parse JSON '{}': {err}", path.display()))
}

fn write_json(output_path: &Path, json: String) -> Result<(), String> {
    fs::write(output_path, json).map_err(|err| {
        format!(
            "Could not write JSON file '{}': {err}",
            output_path.display()
        )
    })
}

fn inspect_commands(parsed: &Value, options: &InspectOptions) -> Result<(), String> {
    let commands = parsed
        .get("debug")
        .and_then(|debug| debug.get("commands"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "debug.commands is missing. Re-run parse with --debug-commands first.".to_string()
        })?;
    let filtered = commands
        .iter()
        .filter(|command| command_matches_inspect_filters(command, options))
        .collect::<Vec<_>>();
    let mut command_counts = HashMap::new();
    let mut parsed_as_counts = HashMap::new();

    for command in &filtered {
        if let Some(command_id) = value_i32(command.get("commandId")) {
            *command_counts.entry(command_id).or_insert(0usize) += 1;
        }
        if let Some(parsed_as) = command.get("parsedAs").and_then(Value::as_str) {
            *parsed_as_counts
                .entry(parsed_as.to_string())
                .or_insert(0usize) += 1;
        }
    }

    println!("Window: {}", inspect_window_label(options));
    println!("Matched commands: {} / {}", filtered.len(), commands.len());
    print_i32_counts("commandId counts", &command_counts);
    print_string_counts("parsedAs counts", &parsed_as_counts);
    println!("Events:");

    for command in filtered.iter().take(options.limit) {
        print_debug_command(command, options.full_hex);
    }

    if filtered.len() > options.limit {
        println!(
            "... {} more command(s), increase --limit to show them",
            filtered.len() - options.limit
        );
    }

    Ok(())
}

fn player_summary(parsed: &Value) -> Result<(), String> {
    let states = parsed
        .get("playerStates")
        .or_else(|| parsed.get("debug").and_then(|debug| debug.get("playerStates")))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "playerStates is missing. Re-run parse with --events or --debug-commands first."
                .to_string()
        })?;

    println!("Command-derived player summary (issued actions only — NOT live game state).");
    for state in states {
        let slot = value_i32(state.get("slotId")).unwrap_or(-1);
        let name = state.get("name").and_then(Value::as_str).unwrap_or("?");
        let civ = state.get("civ").and_then(Value::as_str).unwrap_or("?");
        let counts = state.get("counts");
        let ships = counts
            .and_then(|counts| value_i32(counts.get("shipmentsSent")))
            .unwrap_or(0);
        let techs = counts
            .and_then(|counts| value_i32(counts.get("techsResearched")))
            .unwrap_or(0);
        let units = counts
            .and_then(|counts| value_i32(counts.get("unitsTrainedTotal")))
            .unwrap_or(0);
        let buildings = counts
            .and_then(|counts| value_i32(counts.get("buildingsBuilt")))
            .unwrap_or(0);
        println!("\nslot {slot} {name} [{civ}]  shipments={ships} techs={techs} unitsTrained={units} buildings={buildings}");

        let top_units = state
            .get("unitsTrained")
            .and_then(Value::as_array)
            .map(|tallies| {
                tallies
                    .iter()
                    .take(6)
                    .filter_map(|tally| {
                        let name = tally.get("name").and_then(Value::as_str)?;
                        let count = value_i32(tally.get("count"))?;
                        Some(format!("{name}x{count}"))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        if !top_units.is_empty() {
            println!("  top units: {top_units}");
        }

        let ship_names = derived_names(state.get("shipmentsSent"), 6);
        if !ship_names.is_empty() {
            println!("  shipments: {ship_names}");
        }
        let building_names = derived_names(state.get("buildingsBuilt"), 8);
        if !building_names.is_empty() {
            println!("  buildings: {building_names}");
        }
        if let Some(spent) = state.get("resourcesSpent") {
            let res = |key: &str| spent.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            println!(
                "  resources spent (gross): food={:.0} wood={:.0} gold={:.0} influence={:.0} total={:.0}",
                res("food"),
                res("wood"),
                res("gold"),
                res("influence"),
                res("total")
            );
        }
    }

    if let Some(reason) = states
        .first()
        .and_then(|state| state.get("unavailable"))
        .and_then(|unavailable| unavailable.get("reason"))
        .and_then(Value::as_str)
    {
        println!("\nUnavailable (losses / active counts / resources): {reason}");
    }

    Ok(())
}

fn derived_names(value: Option<&Value>, limit: usize) -> String {
    value
        .and_then(Value::as_array)
        .map(|events| {
            events
                .iter()
                .take(limit)
                .filter_map(|event| event.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn inspect_card_commands(parsed: &Value, actor_slot: Option<i32>) -> Result<(), String> {
    let commands = parsed
        .get("debug")
        .and_then(|debug| debug.get("commands"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "debug.commands is missing. Re-run parse with --debug-commands first.".to_string()
        })?;

    let mut slots: Vec<i32> = commands
        .iter()
        .filter(|command| is_card_related_command(command))
        .filter_map(|command| command_actor_slot(command))
        .collect();
    slots.sort_unstable();
    slots.dedup();

    if let Some(wanted) = actor_slot {
        slots.retain(|slot| *slot == wanted);
    }

    if slots.is_empty() {
        println!("No commandId=2/66 card commands matched the requested filters.");
        return Ok(());
    }

    for slot in slots {
        let name = commands
            .iter()
            .filter(|command| command_actor_slot(command) == Some(slot))
            .find_map(|command| {
                command
                    .get("actor")
                    .and_then(|actor| actor.get("name"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("Unknown");
        println!("Actor: {name} slot={slot}");

        println!("  Deck selections (commandId=66, cardId=-1):");
        let mut any = false;
        for command in commands
            .iter()
            .filter(|command| command_actor_slot(command) == Some(slot))
        {
            if command.get("parsedAs").and_then(Value::as_str) != Some("deck_select_candidate") {
                continue;
            }
            let time_ms = value_i32(command.get("timeMs")).unwrap_or_default();
            let deck_id = command
                .get("decodedFields")
                .and_then(|fields| value_i32(fields.get("deckIdCandidate")))
                .unwrap_or(-1);
            println!("    {} selected deckId={deck_id}", format_time_ms(time_ms));
            any = true;
        }
        if !any {
            println!("    none (active deck must come from a unique parsed default deck)");
        }

        println!("  Card sends (commandId=2, deck index variant):");
        any = false;
        for command in commands
            .iter()
            .filter(|command| command_actor_slot(command) == Some(slot))
        {
            if command.get("parsedAs").and_then(Value::as_str) != Some("card_send_candidate") {
                continue;
            }
            let time_ms = value_i32(command.get("timeMs")).unwrap_or_default();
            let deck_index = command
                .get("decodedFields")
                .and_then(|fields| value_i32(fields.get("deckIndexCandidate")))
                .unwrap_or(-1);
            let deck_match = command
                .get("deckMatch")
                .map(deck_match_label)
                .unwrap_or_else(|| "no deckMatch data".to_string());
            println!(
                "    {} deckIndex={deck_index} {deck_match}",
                format_time_ms(time_ms)
            );
            any = true;
        }
        if !any {
            println!("    none");
        }

        print_actor_deck_candidates(parsed, commands, slot);
        println!();
    }

    println!("System shipment arrival messages (hints only — they do NOT prove which player sent a card; arrivals can lag sends by minutes):");
    let mut any = false;
    if let Some(events) = timeline_events(parsed) {
        for event in events {
            if event.get("type").and_then(Value::as_str) != Some("chat") {
                continue;
            }
            let Some(message) = event
                .get("payload")
                .and_then(|payload| payload.get("message"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !message.contains("Shipment has arrived") {
                continue;
            }
            let time_ms = value_i32(event.get("timeMs")).unwrap_or_default();
            println!("  {} {}", format_time_ms(time_ms), message.trim());
            any = true;
        }
    }
    if !any {
        println!("  none");
    }

    Ok(())
}

fn is_card_related_command(command: &Value) -> bool {
    matches!(
        command.get("parsedAs").and_then(Value::as_str),
        Some("card_send_candidate" | "deck_select_candidate" | "deck_card_add_candidate")
    )
}

fn command_actor_slot(command: &Value) -> Option<i32> {
    command
        .get("actor")
        .and_then(|actor| value_i32(actor.get("slotId")))
}

fn print_actor_deck_candidates(parsed: &Value, commands: &[Value], slot: i32) {
    println!("  Known decks:");
    let mut any = false;

    if let Some(players) = parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array)
    {
        for player in players {
            if value_i32(player.get("slotId")) != Some(slot) {
                continue;
            }
            for deck in player
                .get("initialDecks")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let deck_id = value_i32(deck.get("deckId")).unwrap_or(-1);
                let deck_name = deck
                    .get("deckName")
                    .and_then(Value::as_str)
                    .unwrap_or("Unnamed Deck");
                let is_default = deck
                    .get("isDefault")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let card_ids = deck_card_ids(deck);
                println!(
                    "    deckId={deck_id} name=\"{deck_name}\" default={is_default} cards={} source=parsed_player_deck",
                    card_ids.len()
                );
                any = true;
            }
        }
    }

    let mut command66_decks: HashMap<i32, Vec<i32>> = HashMap::new();
    for command in commands
        .iter()
        .filter(|command| command_actor_slot(command) == Some(slot))
    {
        if command.get("parsedAs").and_then(Value::as_str) != Some("deck_card_add_candidate") {
            continue;
        }
        let fields = command.get("decodedFields");
        let deck_id = fields
            .and_then(|fields| value_i32(fields.get("deckIdCandidate")))
            .unwrap_or(-1);
        let Some(card_id) =
            fields.and_then(|fields| value_i32(fields.get("cardIdCandidate")))
        else {
            continue;
        };
        command66_decks.entry(deck_id).or_default().push(card_id);
    }
    let mut command66_decks: Vec<(i32, Vec<i32>)> = command66_decks.into_iter().collect();
    command66_decks.sort_by_key(|(deck_id, _)| *deck_id);
    for (deck_id, card_ids) in command66_decks {
        println!(
            "    deckId={deck_id} cards={} source=debug_command66_deck_setup rawIds=[{}]",
            card_ids.len(),
            format_i32_list(&card_ids)
        );
        any = true;
    }

    if !any {
        println!("    none");
    }
}

fn compare_summaries_args(args: &[String], start_index: usize) -> Result<(PathBuf, PathBuf), String> {
    let mut a_path = None;
    let mut b_path = None;
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "--a" => {
                a_path = Some(PathBuf::from(value_after(args, index, "--a")?));
                index += 2;
            }
            "--b" => {
                b_path = Some(PathBuf::from(value_after(args, index, "--b")?));
                index += 2;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok((
        a_path.ok_or_else(|| "Missing --a <debug-json>".to_string())?,
        b_path.ok_or_else(|| "Missing --b <debug-json>".to_string())?,
    ))
}

fn import_args(args: &[String], start_index: usize) -> Result<(PathBuf, PathBuf), String> {
    let mut input = None;
    let mut out = None;
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "--input" => {
                input = Some(PathBuf::from(value_after(args, index, "--input")?));
                index += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(value_after(args, index, "--out")?));
                index += 2;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok((
        input.ok_or_else(|| format!("Missing --input <aoe3-companion path>\n\n{}", usage()))?,
        out.unwrap_or_else(|| PathBuf::from("data")),
    ))
}

/// Parse `resolve-card/unit/tech` args: either `--<flag> <id>` or a bare
/// positional id (`resolve-card 1676`). `flag` is e.g. `--card-id`.
fn resolve_id_arg(args: &[String], flag: &str) -> Result<i32, String> {
    let mut id = None;
    let mut index = 1;

    while index < args.len() {
        let arg = args[index].as_str();
        if arg == flag {
            id = Some(parse_i32_arg(args, index, flag)?);
            index += 2;
        } else if id.is_none() {
            id = Some(
                arg.parse::<i32>()
                    .map_err(|err| format!("Invalid id '{arg}': {err}"))?,
            );
            index += 1;
        } else {
            return Err(format!("Unexpected argument '{arg}'\n\n{}", usage()));
        }
    }

    id.ok_or_else(|| format!("Missing {flag} <id>\n\n{}", usage()))
}

fn inspect_card_args(args: &[String], start_index: usize) -> Result<Option<i32>, String> {
    let mut actor_slot = None;
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "--actor-slot" => {
                actor_slot = Some(parse_i32_arg(args, index, "--actor-slot")?);
                index += 2;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok(actor_slot)
}

/// Compare two debug JSONs' command-id histograms. For the death-vs-control
/// experiment: a command id that appears (or jumps) only in the death replay is
/// a candidate outcome/death event. See docs/reverse-engineering/replay-model.md.
fn compare_summaries(
    a_path: &Path,
    a: &Value,
    b_path: &Path,
    b: &Value,
) -> Result<(), String> {
    let a_counts = command_id_counts(a)
        .ok_or_else(|| format!("{}: debug.debugSummary.commandIds missing", a_path.display()))?;
    let b_counts = command_id_counts(b)
        .ok_or_else(|| format!("{}: debug.debugSummary.commandIds missing", b_path.display()))?;

    println!("A: {}", a_path.display());
    println!("B: {}", b_path.display());
    println!(
        "A total commands: {}   B total commands: {}",
        a_counts.values().sum::<i64>(),
        b_counts.values().sum::<i64>()
    );

    let mut ids = a_counts.keys().chain(b_counts.keys()).copied().collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();

    println!("commandId   A      B      delta(B-A)");
    let mut only_in_b = Vec::new();
    for id in ids {
        let a_count = a_counts.get(&id).copied().unwrap_or(0);
        let b_count = b_counts.get(&id).copied().unwrap_or(0);
        let delta = b_count - a_count;
        println!("  {id:>5}   {a_count:>5}  {b_count:>5}   {delta:>+6}");
        if a_count == 0 && b_count > 0 {
            only_in_b.push(id);
        }
    }

    if only_in_b.is_empty() {
        println!("\nNo command id appears only in B. If B is the death replay, that is evidence");
        println!("there is no explicit death/outcome command (deaths are not logged in-file).");
    } else {
        println!("\nCommand ids only in B (candidate outcome events to inspect): {only_in_b:?}");
    }

    Ok(())
}

fn command_id_counts(parsed: &Value) -> Option<HashMap<i32, i64>> {
    parsed
        .get("debug")
        .and_then(|debug| debug.get("debugSummary"))
        .and_then(|summary| summary.get("commandIds"))
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| Some((key.parse::<i32>().ok()?, value.as_i64()?)))
                .collect()
        })
}

fn compare_commands(a: &Value, b: &Value, options: &CompareOptions) -> Result<(), String> {
    let a_command = debug_command_by_offset(a, options.a_offset)?;
    let b_command = debug_command_by_offset(b, options.b_offset)?;

    println!("A: {}", options.a_path.display());
    print_compare_command_summary("A", a_command);
    println!("B: {}", options.b_path.display());
    print_compare_command_summary("B", b_command);
    println!();

    print_decoded_field_diff(a_command, b_command, options.show_same);
    print_raw_field_diff(
        a_command,
        b_command,
        "u32le",
        "i32",
        options.limit,
        options.show_same,
    );
    print_raw_field_diff(
        a_command,
        b_command,
        "u16le",
        "value",
        options.limit,
        options.show_same,
    );

    Ok(())
}

fn dump_decks(parsed: &Value, options: &DumpDecksOptions) -> Result<(), String> {
    let players = parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array)
        .ok_or_else(|| "replay.players is missing or not an array".to_string())?;

    let mut printed_players = 0usize;
    let mut total_matches = 0usize;

    for player in players {
        let slot_id = value_i32(player.get("slotId"));
        if options
            .slot
            .is_some_and(|wanted_slot| Some(wanted_slot) != slot_id)
        {
            continue;
        }

        let decks = player
            .get("initialDecks")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if decks.is_empty() && options.card_id.is_some() {
            continue;
        }

        let matching_deck_count = decks
            .iter()
            .filter(|deck| deck_contains_card_id(deck, options.card_id))
            .count();
        if options.card_id.is_some() && matching_deck_count == 0 {
            continue;
        }

        printed_players += 1;
        let name = player
            .get("playerName")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let civ = player
            .get("civInfo")
            .and_then(|civ| civ.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        println!(
            "Player slot={} name={} civ={} decks={}",
            slot_id
                .map(|slot_id| slot_id.to_string())
                .unwrap_or_else(|| "?".to_string()),
            name,
            civ,
            decks.len()
        );

        for deck in decks {
            let card_ids = deck_card_ids(deck);
            let matches = options
                .card_id
                .map(|card_id| {
                    card_ids
                        .iter()
                        .enumerate()
                        .filter_map(|(index, raw_id)| (*raw_id == card_id).then_some(index))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if options.card_id.is_some() && matches.is_empty() {
                continue;
            }
            total_matches += matches.len();

            let deck_id = value_i32(deck.get("deckId"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string());
            let deck_name = deck
                .get("deckName")
                .and_then(Value::as_str)
                .unwrap_or("Unnamed Deck");
            let marker = if matches.is_empty() {
                String::new()
            } else {
                format!(" matches at indexes {:?}", matches)
            };
            println!(
                "  Deck id={} name=\"{}\" cards={}{}",
                deck_id,
                deck_name,
                card_ids.len(),
                marker
            );
            println!("    rawIds={}", format_i32_list(&card_ids));
        }
    }

    let debug_matches = print_debug_deck_candidates(parsed, options);

    if printed_players == 0 && debug_matches == 0 {
        println!("No deck data matched the requested filters.");
    } else if let Some(card_id) = options.card_id {
        if printed_players > 0 {
            println!("Parsed deck matches for raw card id {card_id}: {total_matches}");
        }
        if debug_matches > 0 {
            println!(
                "Debug deck setup candidate matches for raw card id {card_id}: {debug_matches}"
            );
        }
    }

    Ok(())
}

fn print_debug_deck_candidates(parsed: &Value, options: &DumpDecksOptions) -> usize {
    let Some(commands) = parsed
        .get("debug")
        .and_then(|debug| debug.get("commands"))
        .and_then(Value::as_array)
    else {
        return 0;
    };

    let matches = commands
        .iter()
        .filter(|command| debug_deck_candidate_matches(command, options))
        .collect::<Vec<_>>();

    if matches.is_empty() {
        return 0;
    }

    println!("Debug commandId=66 deck setup candidates:");
    for command in &matches {
        let time_ms = value_i32(command.get("timeMs")).unwrap_or_default();
        let actor = command
            .get("actor")
            .map(actor_label)
            .unwrap_or_else(|| "Unknown".to_string());
        let slot_id = command
            .get("actor")
            .and_then(|actor| value_i32(actor.get("slotId")))
            .map(|slot_id| slot_id.to_string())
            .unwrap_or_else(|| "?".to_string());
        let offset = command
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let fields = command.get("decodedFields");
        let deck_id = fields
            .and_then(|fields| value_i32(fields.get("deckIdCandidate")))
            .unwrap_or_default();
        let card_id = fields
            .and_then(|fields| value_i32(fields.get("cardIdCandidate")))
            .unwrap_or_default();

        println!(
            "  {} actor={} slot={} deckIdCandidate={} cardIdCandidate={} offset={}",
            format_time_ms(time_ms),
            actor,
            slot_id,
            deck_id,
            card_id,
            offset
        );
    }

    matches.len()
}

fn debug_deck_candidate_matches(command: &Value, options: &DumpDecksOptions) -> bool {
    if value_i32(command.get("commandId")) != Some(66) {
        return false;
    }

    let actor_slot = command
        .get("actor")
        .and_then(|actor| value_i32(actor.get("slotId")));
    if options
        .slot
        .is_some_and(|wanted_slot| Some(wanted_slot) != actor_slot)
    {
        return false;
    }

    let Some(card_id) = command
        .get("decodedFields")
        .and_then(|fields| value_i32(fields.get("cardIdCandidate")))
        .filter(|card_id| *card_id > 0)
    else {
        return false;
    };

    options.card_id.is_none_or(|wanted_id| wanted_id == card_id)
}

fn deck_contains_card_id(deck: &Value, card_id: Option<i32>) -> bool {
    match card_id {
        Some(card_id) => deck_card_ids(deck).contains(&card_id),
        None => true,
    }
}

fn deck_card_ids(deck: &Value) -> Vec<i32> {
    deck.get("cards")
        .and_then(Value::as_array)
        .map(|cards| {
            cards
                .iter()
                .filter_map(|card| value_i32(card.get("rawId")))
                .collect::<Vec<_>>()
        })
        .filter(|ids| !ids.is_empty())
        .or_else(|| {
            deck.get("techIds").and_then(Value::as_array).map(|ids| {
                ids.iter()
                    .filter_map(|value| value_i32(Some(value)))
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default()
}

fn format_i32_list(values: &[i32]) -> String {
    values
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn debug_command_by_offset(parsed: &Value, offset: u64) -> Result<&Value, String> {
    let commands = parsed
        .get("debug")
        .and_then(|debug| debug.get("commands"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "debug.commands is missing. Re-run parse with --debug-commands first.".to_string()
        })?;

    commands
        .iter()
        .find(|command| command.get("offset").and_then(Value::as_u64) == Some(offset))
        .ok_or_else(|| format!("Could not find debug command with offset {offset}"))
}

fn print_compare_command_summary(label: &str, command: &Value) {
    let time_ms = value_i32(command.get("timeMs")).unwrap_or_default();
    let actor = command
        .get("actor")
        .map(actor_label)
        .unwrap_or_else(|| "Unknown".to_string());
    let command_id = value_i32(command.get("commandId")).unwrap_or_default();
    let command_name = command
        .get("commandName")
        .and_then(Value::as_str)
        .unwrap_or("unknown_command");
    let parsed_as = command
        .get("parsedAs")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let offset = command
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let length = command
        .get("length")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    println!(
        "{label}: time={} actor={} commandId={} commandName={} parsedAs={} offset={} len={}",
        format_time_ms(time_ms),
        actor,
        command_id,
        command_name,
        parsed_as,
        offset,
        length
    );
}

fn print_decoded_field_diff(a: &Value, b: &Value, show_same: bool) {
    let a_fields = a
        .get("decodedFields")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let b_fields = b
        .get("decodedFields")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut keys = a_fields
        .keys()
        .chain(b_fields.keys())
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();

    println!("decodedFields:");
    let mut printed = 0usize;
    for key in keys {
        let a_value = a_fields.get(&key).and_then(|value| value.as_i64());
        let b_value = b_fields.get(&key).and_then(|value| value.as_i64());
        let same = a_value == b_value;
        if same && !show_same {
            continue;
        }
        println!(
            "  {key}: {} A={} B={}",
            diff_label(same),
            format_optional_i64(a_value),
            format_optional_i64(b_value)
        );
        printed += 1;
    }
    if printed == 0 {
        println!("  no differences");
    }
}

fn print_raw_field_diff(
    a: &Value,
    b: &Value,
    field_name: &str,
    value_key: &str,
    limit: usize,
    show_same: bool,
) {
    let a_values = raw_field_values(a, field_name, value_key);
    let b_values = raw_field_values(b, field_name, value_key);
    let mut offsets = a_values
        .keys()
        .chain(b_values.keys())
        .copied()
        .collect::<Vec<_>>();
    offsets.sort();
    offsets.dedup();

    println!("rawFields.{field_name}:");
    let mut printed = 0usize;
    for offset in offsets {
        let a_value = a_values.get(&offset).copied();
        let b_value = b_values.get(&offset).copied();
        let same = a_value == b_value;
        if same && !show_same {
            continue;
        }
        println!(
            "  offset {offset:03}: {} A={} B={}",
            diff_label(same),
            format_optional_i64(a_value),
            format_optional_i64(b_value)
        );
        printed += 1;
        if printed >= limit {
            println!("  ... increase --limit to show more fields");
            break;
        }
    }
    if printed == 0 {
        println!("  no differences");
    }
}

fn raw_field_values(command: &Value, field_name: &str, value_key: &str) -> HashMap<usize, i64> {
    command
        .get("rawFields")
        .and_then(|fields| fields.get(field_name))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let offset = item.get("offset")?.as_u64()? as usize;
                    let value = item.get(value_key)?.as_i64()?;
                    Some((offset, value))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn diff_label(same: bool) -> &'static str {
    if same {
        "same"
    } else {
        "DIFF"
    }
}

fn format_optional_i64(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string())
}

fn command_matches_inspect_filters(command: &Value, options: &InspectOptions) -> bool {
    let time_ms = value_i32(command.get("timeMs"));
    if let Some(from) = options.from {
        if time_ms.is_none_or(|time_ms| time_ms < from) {
            return false;
        }
    }
    if let Some(to) = options.to {
        if time_ms.is_none_or(|time_ms| time_ms > to) {
            return false;
        }
    }
    if let Some(command_id) = options.command_id {
        if value_i32(command.get("commandId")) != Some(command_id) {
            return false;
        }
    }
    if let Some(actor_slot) = options.actor_slot {
        let slot_id = command
            .get("actor")
            .and_then(|actor| value_i32(actor.get("slotId")));
        if slot_id != Some(actor_slot) {
            return false;
        }
    }
    if let Some(parsed_as) = &options.parsed_as {
        if command.get("parsedAs").and_then(Value::as_str) != Some(parsed_as.as_str()) {
            return false;
        }
    }

    true
}

fn inspect_window_label(options: &InspectOptions) -> String {
    match (options.from, options.to) {
        (Some(from), Some(to)) => format!("{} - {}", format_time_ms(from), format_time_ms(to)),
        (Some(from), None) => format!("from {}", format_time_ms(from)),
        (None, Some(to)) => format!("through {}", format_time_ms(to)),
        (None, None) => "all commands".to_string(),
    }
}

fn print_i32_counts(label: &str, counts: &HashMap<i32, usize>) {
    println!("{label}:");
    if counts.is_empty() {
        println!("  none");
        return;
    }

    let mut values = counts.iter().collect::<Vec<_>>();
    values.sort_by(|(left_id, left_count), (right_id, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_id.cmp(right_id))
    });

    for (key, value) in values {
        println!("  {key}: {value}");
    }
}

fn print_string_counts(label: &str, counts: &HashMap<String, usize>) {
    println!("{label}:");
    if counts.is_empty() {
        println!("  none");
        return;
    }

    let mut values = counts.iter().collect::<Vec<_>>();
    values.sort_by(|(left_key, left_count), (right_key, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_key.cmp(right_key))
    });

    for (key, value) in values {
        println!("  {key}: {value}");
    }
}

fn print_debug_command(command: &Value, full_hex: bool) {
    let time_ms = value_i32(command.get("timeMs")).unwrap_or_default();
    let actor = command
        .get("actor")
        .map(actor_label)
        .unwrap_or_else(|| "Unknown".to_string());
    let command_id = value_i32(command.get("commandId")).unwrap_or_default();
    let command_name = command
        .get("commandName")
        .and_then(Value::as_str)
        .unwrap_or("unknown_command");
    let decoded = command
        .get("decoded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let offset = command
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let length = command
        .get("length")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let parsed_as = command
        .get("parsedAs")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let full_hex_value = command.get("hex").and_then(Value::as_str);
    let hex_label = if full_hex && full_hex_value.is_some() {
        "hex"
    } else {
        "hexPreview"
    };
    let hex_value = full_hex_value
        .filter(|_| full_hex)
        .or_else(|| command.get("hexPreview").and_then(Value::as_str))
        .unwrap_or("");

    println!(
        "{} actor={} commandId={} commandName={} decoded={} offset={} len={} parsedAs={}",
        format_time_ms(time_ms),
        actor,
        command_id,
        command_name,
        decoded,
        offset,
        length,
        parsed_as
    );
    println!("  {hex_label}={hex_value}");
    if let Some(fields) = command.get("decodedFields").and_then(Value::as_object) {
        if !fields.is_empty() {
            let mut values = fields.iter().collect::<Vec<_>>();
            values.sort_by_key(|(key, _)| key.as_str());
            let label = values
                .into_iter()
                .map(|(key, value)| format!("{key}={}", value_i32(Some(value)).unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(", ");
            println!("  decodedFields={label}");
        }
    }
    if let Some(matches) = command.get("deckMatches").and_then(Value::as_array) {
        if !matches.is_empty() {
            let label = matches
                .iter()
                .filter_map(|deck_match| {
                    let deck_id = value_i32(deck_match.get("deckId"))?;
                    let deck_name = deck_match
                        .get("deckName")
                        .and_then(Value::as_str)
                        .unwrap_or("Unnamed Deck");
                    let card_index = deck_match.get("cardIndex").and_then(Value::as_u64)?;
                    let raw_id = value_i32(deck_match.get("rawId"))?;
                    Some(format!(
                        "deckId={deck_id} deck=\"{deck_name}\" cardIndex={card_index} rawId={raw_id}"
                    ))
                })
                .collect::<Vec<_>>()
                .join("; ");
            if !label.is_empty() {
                println!("  deckMatches={label}");
            }
        }
    }
    if let Some(deck_match) = command.get("deckMatch") {
        println!("  deckMatch={}", deck_match_label(deck_match));
    }
    if let Some(name) = command
        .get("unit")
        .and_then(|unit| unit.get("displayName"))
        .and_then(Value::as_str)
    {
        println!("  unit={name}");
    }
    if let Some(name) = command
        .get("tech")
        .and_then(|tech| tech.get("displayName"))
        .and_then(Value::as_str)
    {
        println!("  tech={name}");
    }
    if let Some(name) = command
        .get("building")
        .and_then(|building| building.get("displayName"))
        .and_then(Value::as_str)
    {
        println!("  building={name}");
    }
    if full_hex {
        print_raw_u32_fields(command);
    }
}

fn deck_match_label(deck_match: &Value) -> String {
    let matched = deck_match
        .get("matched")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if matched {
        let card_id = value_i32(deck_match.get("cardIdCandidate")).unwrap_or(-1);
        let deck_id = value_i32(deck_match.get("activeDeckId"))
            .map(|id| id.to_string())
            .unwrap_or_else(|| "?".to_string());
        let deck_name = deck_match
            .get("deckName")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let source = deck_match
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let confidence = deck_match
            .get("confidence")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let card_name = deck_match
            .get("card")
            .and_then(|card| card.get("displayName"))
            .and_then(Value::as_str)
            .map(|name| format!(" card=\"{name}\""))
            .unwrap_or_default();
        format!(
            "matched cardId={card_id}{card_name} deckId={deck_id} deck=\"{deck_name}\" source={source} confidence={confidence}"
        )
    } else {
        let reason = deck_match
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        format!("unmatched ({reason})")
    }
}

fn print_raw_u32_fields(command: &Value) {
    let Some(values) = command
        .get("rawFields")
        .and_then(|fields| fields.get("u32le"))
        .and_then(Value::as_array)
    else {
        return;
    };

    let label = values
        .iter()
        .take(32)
        .filter_map(|field| {
            let offset = field.get("offset")?.as_u64()?;
            let value = field.get("i32").and_then(Value::as_i64)?;
            Some(format!("{offset}:{value}"))
        })
        .collect::<Vec<_>>()
        .join(", ");

    if !label.is_empty() {
        println!("  rawFields.u32le={label}");
    }
}

fn actor_label(actor: &Value) -> String {
    match actor.get("kind").and_then(Value::as_str) {
        Some("system") => "System".to_string(),
        Some("player") => actor
            .get("name")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| value_i32(actor.get("slotId")).map(|slot_id| format!("Slot {slot_id}")))
            .unwrap_or_else(|| "Player".to_string()),
        Some("unknown") => value_i32(actor.get("slotId"))
            .map(|slot_id| format!("Unknown slot {slot_id}"))
            .unwrap_or_else(|| "Unknown".to_string()),
        _ => "Unknown".to_string(),
    }
}

fn format_time_ms(time_ms: i32) -> String {
    let time_ms = time_ms.max(0);
    let minutes = time_ms / 60_000;
    let seconds = (time_ms % 60_000) / 1_000;
    let millis = time_ms % 1_000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
}

struct Cli {
    command: Command,
}

enum Command {
    Parse {
        replay_path: PathBuf,
        output_path: PathBuf,
        debug_commands: bool,
        experimental_shipments: bool,
        events: bool,
    },
    Normalize {
        parsed_path: PathBuf,
        output_path: PathBuf,
    },
    Validate {
        parsed_path: PathBuf,
    },
    InspectCommands {
        parsed_path: PathBuf,
        options: InspectOptions,
    },
    InspectCardCommands {
        parsed_path: PathBuf,
        actor_slot: Option<i32>,
    },
    CompareCommands {
        options: CompareOptions,
    },
    CompareSummaries {
        a_path: PathBuf,
        b_path: PathBuf,
    },
    DumpDecks {
        parsed_path: PathBuf,
        options: DumpDecksOptions,
    },
    PlayerSummary {
        parsed_path: PathBuf,
    },
    ResolveCard {
        card_id: i32,
    },
    ResolveUnit {
        unit_id: i32,
    },
    ResolveTech {
        tech_id: i32,
    },
    ResolveBuilding {
        building_id: i32,
    },
    ImportAoe3Companion {
        input: PathBuf,
        out: PathBuf,
    },
    ValidateCorpus {
        dir: PathBuf,
    },
    Capture {
        offsets_path: PathBuf,
        hz: u32,
        duration_s: u64,
        output_path: Option<PathBuf>,
    },
    MergeCapture {
        replay_path: PathBuf,
        capture_path: PathBuf,
        offset_ms: i64,
        output_path: Option<PathBuf>,
    },
}

#[derive(Debug)]
struct InspectOptions {
    from: Option<i32>,
    to: Option<i32>,
    command_id: Option<i32>,
    actor_slot: Option<i32>,
    parsed_as: Option<String>,
    limit: usize,
    full_hex: bool,
}

#[derive(Debug)]
struct CompareOptions {
    a_path: PathBuf,
    a_offset: u64,
    b_path: PathBuf,
    b_offset: u64,
    limit: usize,
    show_same: bool,
}

#[derive(Debug)]
struct DumpDecksOptions {
    slot: Option<i32>,
    card_id: Option<i32>,
}

impl Cli {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        if args.is_empty() || args.iter().any(|arg| arg == "-h" || arg == "--help") {
            return Err(usage());
        }

        let command_name = args.first().map(String::as_str).ok_or_else(usage)?;
        let command = match command_name {
            "compare-commands" => Command::CompareCommands {
                options: compare_args(&args, 1)?,
            },
            "compare-summaries" => {
                let (a_path, b_path) = compare_summaries_args(&args, 1)?;
                Command::CompareSummaries { a_path, b_path }
            }
            "dump-decks" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                let input_path = PathBuf::from(input_arg.as_str());
                Command::DumpDecks {
                    parsed_path: input_path,
                    options: dump_decks_args(&args, 2)?,
                }
            }
            "player-summary" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                if args.len() > 2 {
                    return Err(format!("Unexpected argument '{}'\n\n{}", args[2], usage()));
                }
                Command::PlayerSummary {
                    parsed_path: PathBuf::from(input_arg.as_str()),
                }
            }
            "resolve-card" => Command::ResolveCard {
                card_id: resolve_id_arg(&args, "--card-id")?,
            },
            "resolve-unit" => Command::ResolveUnit {
                unit_id: resolve_id_arg(&args, "--unit-id")?,
            },
            "resolve-tech" => Command::ResolveTech {
                tech_id: resolve_id_arg(&args, "--tech-id")?,
            },
            "resolve-building" => Command::ResolveBuilding {
                building_id: resolve_id_arg(&args, "--building-id")?,
            },
            "import-aoe3-companion" => {
                let (input, out) = import_args(&args, 1)?;
                Command::ImportAoe3Companion { input, out }
            }
            "validate-corpus" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                if args.len() > 2 {
                    return Err(format!("Unexpected argument '{}'\n\n{}", args[2], usage()));
                }
                Command::ValidateCorpus {
                    dir: PathBuf::from(input_arg.as_str()),
                }
            }
            "capture" => {
                let mut offsets_path: Option<PathBuf> = None;
                let mut output_path: Option<PathBuf> = None;
                let mut hz: u32 = 2;
                let mut duration_s: u64 = 600;
                let mut i = 1;
                while i < args.len() {
                    match args[i].as_str() {
                        "--offsets" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            offsets_path = Some(PathBuf::from(v.as_str()));
                            i += 2;
                        }
                        "-o" | "--output" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            output_path = Some(PathBuf::from(v.as_str()));
                            i += 2;
                        }
                        "--hz" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            hz = v
                                .parse()
                                .map_err(|_| format!("--hz expects an integer, got '{v}'"))?;
                            i += 2;
                        }
                        "--duration" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            duration_s = v
                                .parse()
                                .map_err(|_| format!("--duration expects seconds, got '{v}'"))?;
                            i += 2;
                        }
                        other => {
                            return Err(format!("Unexpected argument '{other}'\n\n{}", usage()));
                        }
                    }
                }
                let offsets_path = offsets_path
                    .ok_or_else(|| format!("capture requires --offsets <file>\n\n{}", usage()))?;
                Command::Capture {
                    offsets_path,
                    hz,
                    duration_s,
                    output_path,
                }
            }
            "merge-capture" => {
                let mut replay_path: Option<PathBuf> = None;
                let mut capture_path: Option<PathBuf> = None;
                let mut output_path: Option<PathBuf> = None;
                let mut offset_ms: i64 = 0;
                let mut i = 1;
                while i < args.len() {
                    match args[i].as_str() {
                        "--replay" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            replay_path = Some(PathBuf::from(v.as_str()));
                            i += 2;
                        }
                        "--capture" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            capture_path = Some(PathBuf::from(v.as_str()));
                            i += 2;
                        }
                        "--offset-ms" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            offset_ms = v
                                .parse()
                                .map_err(|_| format!("--offset-ms expects an integer, got '{v}'"))?;
                            i += 2;
                        }
                        "-o" | "--output" => {
                            let v = args.get(i + 1).ok_or_else(usage)?;
                            output_path = Some(PathBuf::from(v.as_str()));
                            i += 2;
                        }
                        other => {
                            return Err(format!("Unexpected argument '{other}'\n\n{}", usage()));
                        }
                    }
                }
                let replay_path = replay_path
                    .ok_or_else(|| format!("merge-capture requires --replay <file>\n\n{}", usage()))?;
                let capture_path = capture_path.ok_or_else(|| {
                    format!("merge-capture requires --capture <file>\n\n{}", usage())
                })?;
                Command::MergeCapture {
                    replay_path,
                    capture_path,
                    offset_ms,
                    output_path,
                }
            }
            "parse" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                let input_path = PathBuf::from(input_arg.as_str());
                let (output_path, debug_commands, experimental_shipments, events) =
                    parse_args(&args, 2, default_output_path(command_name, &input_path))?;
                Command::Parse {
                    replay_path: input_path.clone(),
                    output_path,
                    debug_commands,
                    experimental_shipments,
                    events,
                }
            }
            "normalize" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                let input_path = PathBuf::from(input_arg.as_str());
                Command::Normalize {
                    parsed_path: input_path.clone(),
                    output_path: output_path_arg(
                        &args,
                        2,
                        default_output_path(command_name, &input_path),
                    )?,
                }
            }
            "validate" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                let input_path = PathBuf::from(input_arg.as_str());
                if args.len() > 2 {
                    return Err(format!("Unexpected argument '{}'\n\n{}", args[2], usage()));
                }
                Command::Validate {
                    parsed_path: input_path,
                }
            }
            "inspect-commands" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                let input_path = PathBuf::from(input_arg.as_str());
                Command::InspectCommands {
                    parsed_path: input_path,
                    options: inspect_args(&args, 2)?,
                }
            }
            "inspect-card-commands" => {
                let input_arg = args.get(1).ok_or_else(usage)?;
                let input_path = PathBuf::from(input_arg.as_str());
                Command::InspectCardCommands {
                    parsed_path: input_path,
                    actor_slot: inspect_card_args(&args, 2)?,
                }
            }
            _ => return Err(usage()),
        };

        Ok(Self { command })
    }
}

fn compare_args(args: &[String], start_index: usize) -> Result<CompareOptions, String> {
    let mut a_path = None;
    let mut a_offset = None;
    let mut b_path = None;
    let mut b_offset = None;
    let mut limit = 64usize;
    let mut show_same = false;
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "--a" => {
                a_path = Some(PathBuf::from(value_after(args, index, "--a")?));
                index += 2;
            }
            "--a-offset" | "--offset-a" => {
                a_offset = Some(parse_u64_arg(args, index, args[index].as_str())?);
                index += 2;
            }
            "--b" => {
                b_path = Some(PathBuf::from(value_after(args, index, "--b")?));
                index += 2;
            }
            "--b-offset" | "--offset-b" => {
                b_offset = Some(parse_u64_arg(args, index, args[index].as_str())?);
                index += 2;
            }
            "--limit" => {
                limit = value_after(args, index, "--limit")?
                    .parse::<usize>()
                    .map_err(|err| format!("Invalid --limit value: {err}"))?;
                index += 2;
            }
            "--show-same" => {
                show_same = true;
                index += 1;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok(CompareOptions {
        a_path: a_path.ok_or_else(|| "Missing --a <debug-json>".to_string())?,
        a_offset: a_offset.ok_or_else(|| "Missing --a-offset <offset>".to_string())?,
        b_path: b_path.ok_or_else(|| "Missing --b <debug-json>".to_string())?,
        b_offset: b_offset.ok_or_else(|| "Missing --b-offset <offset>".to_string())?,
        limit,
        show_same,
    })
}

fn dump_decks_args(args: &[String], start_index: usize) -> Result<DumpDecksOptions, String> {
    let mut options = DumpDecksOptions {
        slot: None,
        card_id: None,
    };
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "--slot" => {
                options.slot = Some(parse_i32_arg(args, index, "--slot")?);
                index += 2;
            }
            "--card-id" => {
                options.card_id = Some(parse_i32_arg(args, index, "--card-id")?);
                index += 2;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok(options)
}

fn inspect_args(args: &[String], start_index: usize) -> Result<InspectOptions, String> {
    let mut options = InspectOptions {
        from: None,
        to: None,
        command_id: None,
        actor_slot: None,
        parsed_as: None,
        limit: 50,
        full_hex: false,
    };
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "--from" => {
                options.from = Some(parse_i32_arg(args, index, "--from")?);
                index += 2;
            }
            "--to" => {
                options.to = Some(parse_i32_arg(args, index, "--to")?);
                index += 2;
            }
            "--command-id" => {
                options.command_id = Some(parse_i32_arg(args, index, "--command-id")?);
                index += 2;
            }
            "--actor-slot" => {
                options.actor_slot = Some(parse_i32_arg(args, index, "--actor-slot")?);
                index += 2;
            }
            "--parsed-as" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "Missing value after --parsed-as".to_string())?;
                options.parsed_as = Some(value.clone());
                index += 2;
            }
            "--limit" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "Missing value after --limit".to_string())?
                    .parse::<usize>()
                    .map_err(|err| format!("Invalid --limit value: {err}"))?;
                options.limit = value;
                index += 2;
            }
            "--full-hex" => {
                options.full_hex = true;
                index += 1;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    if let (Some(from), Some(to)) = (options.from, options.to) {
        if from > to {
            return Err("--from must be less than or equal to --to".to_string());
        }
    }

    Ok(options)
}

fn parse_i32_arg(args: &[String], index: usize, name: &str) -> Result<i32, String> {
    value_after(args, index, name)?
        .parse::<i32>()
        .map_err(|err| format!("Invalid {name} value: {err}"))
}

fn parse_u64_arg(args: &[String], index: usize, name: &str) -> Result<u64, String> {
    value_after(args, index, name)?
        .parse::<u64>()
        .map_err(|err| format!("Invalid {name} value: {err}"))
}

fn value_after(args: &[String], index: usize, name: &str) -> Result<String, String> {
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("Missing value after {name}"))
}

fn output_path_arg(
    args: &[String],
    start_index: usize,
    default_output_path: PathBuf,
) -> Result<PathBuf, String> {
    let mut output_path = default_output_path;
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "-o" | "--output" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "Missing value after -o/--output".to_string())?;
                output_path = PathBuf::from(value.as_str());
                index += 2;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok(output_path)
}

fn parse_args(
    args: &[String],
    start_index: usize,
    default_output_path: PathBuf,
) -> Result<(PathBuf, bool, bool, bool), String> {
    let mut output_path = default_output_path;
    let mut debug_commands = false;
    let mut experimental_shipments = false;
    // Verified gameplay events (research/train/build/age-up + playerStates) are
    // emitted by default. `--no-events` opts out for a minimal chat+resign JSON.
    let mut events = true;
    let mut index = start_index;

    while index < args.len() {
        match args[index].as_str() {
            "-o" | "--output" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "Missing value after -o/--output".to_string())?;
                output_path = PathBuf::from(value.as_str());
                index += 2;
            }
            "--debug-commands" => {
                debug_commands = true;
                index += 1;
            }
            "--experimental-shipments" => {
                experimental_shipments = true;
                index += 1;
            }
            "--events" => {
                events = true;
                index += 1;
            }
            "--no-events" => {
                events = false;
                index += 1;
            }
            other => return Err(format!("Unexpected argument '{other}'\n\n{}", usage())),
        }
    }

    Ok((output_path, debug_commands, experimental_shipments, events))
}

fn normalize_parsed_json(mut parsed: Value) -> Result<Value, String> {
    if parsed
        .get("timeline")
        .and_then(|timeline| timeline.get("events"))
        .and_then(Value::as_array)
        .is_some()
    {
        parsed["schemaVersion"] = json!(1);
        migrate_timeline_actors(&mut parsed)?;
        add_summary(&mut parsed)?;
        add_result(&mut parsed);
        return Ok(parsed);
    }

    let commands = parsed.get("commands").cloned();
    let command_parse_error = if commands.is_none() || commands == Some(Value::Null) {
        Some("Command data was unavailable in the source parsed JSON".to_string())
    } else {
        None
    };

    let mut events = Vec::new();
    let mut source_index = 0usize;

    if let Some(chat) = commands
        .as_ref()
        .and_then(|commands| commands.get("chat"))
        .and_then(Value::as_array)
    {
        for message in chat {
            let time = value_i32(message.get("time")).unwrap_or_default();
            let from_id = value_i32(message.get("fromId")).unwrap_or_default();
            events.push((
                time,
                0u8,
                source_index,
                json!({
                    "id": "",
                    "type": "chat",
                    "time": time,
                    "timeMs": time,
                    "actor": actor_json(&parsed, from_id, true),
                    "payload": {
                        "kind": "chat",
                        "toId": value_i32(message.get("toId")).unwrap_or_default(),
                        "message": message.get("message").and_then(Value::as_str).unwrap_or_default(),
                    }
                }),
            ));
            source_index += 1;
        }
    }

    if let Some(resigns) = commands
        .as_ref()
        .and_then(|commands| commands.get("resigns"))
        .and_then(Value::as_array)
    {
        for resign in resigns {
            let time = value_i32(resign.get("time")).unwrap_or_default();
            let slot_id = value_i32(resign.get("slotId")).unwrap_or_default();
            events.push((
                time,
                1u8,
                source_index,
                json!({
                    "id": "",
                    "type": "resign",
                    "time": time,
                    "timeMs": time,
                    "actor": actor_json(&parsed, slot_id, false),
                    "payload": {
                        "kind": "resign"
                    }
                }),
            ));
            source_index += 1;
        }
    }

    events.sort_by_key(|(time, event_order, source_index, _)| (*time, *event_order, *source_index));

    let events: Vec<Value> = events
        .into_iter()
        .enumerate()
        .map(|(index, (_, _, _, mut event))| {
            event["id"] = json!(format!("event-{:06}", index + 1));
            event
        })
        .collect();

    let timeline = match command_parse_error {
        Some(error) => json!({ "events": events, "commandParseError": error }),
        None => json!({ "events": events }),
    };

    parsed
        .as_object_mut()
        .ok_or_else(|| "Expected top-level JSON object".to_string())?
        .remove("commands");
    parsed["schemaVersion"] = json!(1);
    parsed["timeline"] = timeline;
    add_summary(&mut parsed)?;
    add_result(&mut parsed);

    Ok(parsed)
}

fn migrate_timeline_actors(parsed: &mut Value) -> Result<(), String> {
    let player_names = player_names_by_slot(parsed);
    let Some(events) = parsed
        .get_mut("timeline")
        .and_then(|timeline| timeline.get_mut("events"))
        .and_then(Value::as_array_mut)
    else {
        return Err("Expected normalized JSON with top-level timeline.events array".to_string());
    };

    for event in events {
        let event_type = event.get("type").and_then(Value::as_str);
        let existing_actor = event.get("actor");
        let kind = existing_actor
            .and_then(|actor| actor.get("kind"))
            .and_then(Value::as_str);
        let slot_id = existing_actor
            .and_then(|actor| value_i32(actor.get("slotId")))
            .or_else(|| existing_actor.and_then(|actor| value_i32(actor.get("playerId"))));
        let zero_is_system = event_type == Some("chat");
        let actor = match (kind, slot_id) {
            (Some("system"), _) => system_actor_json(),
            (Some("player"), Some(slot_id)) | (_, Some(slot_id))
                if player_names.contains_key(&slot_id) =>
            {
                player_actor_json(slot_id, player_names.get(&slot_id).cloned().flatten())
            }
            (_, Some(slot_id)) if zero_is_system && slot_id <= 0 => system_actor_json(),
            (_, Some(slot_id)) => unknown_actor_json(Some(slot_id)),
            _ => unknown_actor_json(None),
        };

        event["actor"] = actor;
    }

    Ok(())
}

fn add_summary(parsed: &mut Value) -> Result<(), String> {
    let (
        event_count,
        chat_count,
        resign_count,
        shipment_count,
        shipment_confirmed_count,
        shipment_candidate_count,
    ) = event_counts(parsed)?;
    let player_count = parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let team_count = parsed
        .get("replay")
        .and_then(|replay| replay.get("teams"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    parsed["summary"] = json!({
        "eventCount": event_count,
        "chatCount": chat_count,
        "resignCount": resign_count,
        "shipmentCount": shipment_count,
        "shipmentConfirmedCount": shipment_confirmed_count,
        "shipmentCandidateCount": shipment_candidate_count,
        "playerCount": player_count,
        "teamCount": team_count
    });

    Ok(())
}

fn add_result(parsed: &mut Value) {
    parsed["result"] = infer_result_json(parsed);
}

fn event_counts(parsed: &Value) -> Result<(usize, usize, usize, usize, usize, usize), String> {
    let events = timeline_events(parsed).ok_or_else(|| {
        "Expected normalized JSON with top-level timeline.events array".to_string()
    })?;
    let mut chat_count = 0usize;
    let mut resign_count = 0usize;
    let mut shipment_count = 0usize;
    let mut shipment_confirmed_count = 0usize;
    let mut shipment_candidate_count = 0usize;

    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("chat") => chat_count += 1,
            Some("resign") => resign_count += 1,
            Some("shipment") => {
                shipment_count += 1;
                match event
                    .get("payload")
                    .and_then(|payload| payload.get("status"))
                    .and_then(Value::as_str)
                {
                    Some("confirmed") => shipment_confirmed_count += 1,
                    _ => shipment_candidate_count += 1,
                }
            }
            _ => {}
        }
    }

    Ok((
        events.len(),
        chat_count,
        resign_count,
        shipment_count,
        shipment_confirmed_count,
        shipment_candidate_count,
    ))
}

fn actor_json(parsed: &Value, slot_id: i32, zero_is_system: bool) -> Value {
    if zero_is_system && slot_id <= 0 {
        return system_actor_json();
    }

    match player_name(parsed, slot_id) {
        Some(name) => player_actor_json(slot_id, Some(name)),
        None if team_member_slot_ids(parsed).contains(&slot_id) => {
            player_actor_json(slot_id, participant_name_from_team(parsed, slot_id))
        }
        None => unknown_actor_json(Some(slot_id)),
    }
}

fn system_actor_json() -> Value {
    json!({
        "kind": "system",
        "slotId": null,
        "playerId": null,
        "name": "System"
    })
}

fn player_actor_json(slot_id: i32, name: Option<String>) -> Value {
    json!({
        "kind": "player",
        "slotId": slot_id,
        "playerId": slot_id,
        "name": name
    })
}

fn unknown_actor_json(slot_id: Option<i32>) -> Value {
    json!({
        "kind": "unknown",
        "slotId": slot_id,
        "playerId": null,
        "name": null
    })
}

fn player_name(parsed: &Value, slot_id: i32) -> Option<String> {
    parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array)?
        .iter()
        .find(|player| value_i32(player.get("slotId")) == Some(slot_id))
        .and_then(|player| player.get("playerName"))
        .and_then(Value::as_str)
        .map(String::from)
}

fn player_names_by_slot(parsed: &Value) -> HashMap<i32, Option<String>> {
    let mut names: HashMap<i32, Option<String>> = parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array)
        .map(|players| {
            players
                .iter()
                .filter_map(|player| {
                    let slot_id = value_i32(player.get("slotId"))?;
                    let name = player
                        .get("playerName")
                        .and_then(Value::as_str)
                        .map(String::from);
                    Some((slot_id, name))
                })
                .collect()
        })
        .unwrap_or_default();

    for slot_id in team_member_slot_ids(parsed) {
        names
            .entry(slot_id)
            .or_insert_with(|| participant_name_from_team(parsed, slot_id));
    }

    names
}

fn participant_name_from_team(parsed: &Value, slot_id: i32) -> Option<String> {
    parsed
        .get("replay")
        .and_then(|replay| replay.get("teams"))
        .and_then(Value::as_array)?
        .iter()
        .find(|team| {
            team.get("members")
                .and_then(Value::as_array)
                .is_some_and(|members| {
                    members
                        .iter()
                        .any(|member| value_i32(Some(member)) == Some(slot_id))
                })
        })
        .and_then(|team| team.get("name"))
        .and_then(Value::as_str)
        .map(|name| {
            name.strip_prefix("Team ")
                .unwrap_or(name)
                .trim()
                .to_string()
        })
        .filter(|name| !name.is_empty())
}

fn replay_slot_ids(parsed: &Value) -> HashSet<i32> {
    let mut slots = parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array)
        .map(|players| {
            players
                .iter()
                .filter_map(|player| value_i32(player.get("slotId")))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if slots.is_empty() {
        slots.extend(team_member_slot_ids(parsed));
    }

    slots
}

fn team_member_slot_ids(parsed: &Value) -> HashSet<i32> {
    parsed
        .get("replay")
        .and_then(|replay| replay.get("teams"))
        .and_then(Value::as_array)
        .map(|teams| {
            teams
                .iter()
                .flat_map(|team| {
                    team.get("members")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|member| value_i32(Some(member)))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn infer_result_json(parsed: &Value) -> Value {
    let player_slots = replay_slot_ids(parsed);
    let resigned_slots = timeline_events(parsed)
        .map(|events| {
            events
                .iter()
                .filter(|event| event.get("type").and_then(Value::as_str) == Some("resign"))
                .filter_map(|event| {
                    let actor = event.get("actor")?;
                    if actor.get("kind").and_then(Value::as_str) != Some("player") {
                        return None;
                    }
                    value_i32(actor.get("slotId"))
                })
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let teams = parsed
        .get("replay")
        .and_then(|replay| replay.get("teams"))
        .and_then(Value::as_array)
        .map(|teams| {
            teams
                .iter()
                .filter_map(|team| {
                    let team_id = value_i32(team.get("id"))?;
                    let members = team
                        .get("members")
                        .and_then(Value::as_array)?
                        .iter()
                        .filter_map(|member| value_i32(Some(member)))
                        .filter(|slot_id| player_slots.contains(slot_id))
                        .collect::<Vec<_>>();
                    (!members.is_empty()).then_some((team_id, members))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if teams.len() < 2 {
        return result_json(
            false,
            "low",
            Vec::new(),
            Vec::new(),
            "Could not infer winner without at least two valid teams",
        );
    }

    let mut losing_teams = Vec::new();
    let mut remaining_teams = Vec::new();

    for (team_id, members) in teams {
        if members
            .iter()
            .all(|slot_id| resigned_slots.contains(slot_id))
        {
            losing_teams.push(team_id);
        } else {
            remaining_teams.push(team_id);
        }
    }

    if losing_teams.is_empty() {
        return result_json(
            false,
            "low",
            Vec::new(),
            Vec::new(),
            "Could not infer winner because no full team resigned",
        );
    }

    if remaining_teams.len() != 1 {
        return result_json(
            false,
            "low",
            Vec::new(),
            losing_teams,
            "Could not infer winner because resign events leave zero or multiple teams active",
        );
    }

    let reason = format!(
        "All non-observer players from team(s) {} resigned",
        losing_teams
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    result_json(true, "medium", remaining_teams, losing_teams, &reason)
}

fn result_json(
    inferred: bool,
    confidence: &str,
    winning_teams: Vec<i32>,
    losing_teams: Vec<i32>,
    reason: &str,
) -> Value {
    json!({
        "inferred": inferred,
        "confidence": confidence,
        "winningTeams": winning_teams,
        "losingTeams": losing_teams,
        "reason": reason
    })
}

fn validate_parsed_json(parsed: &Value) -> ValidationReport {
    let mut report = ValidationReport::default();

    match value_i32(parsed.get("schemaVersion")) {
        Some(1) => report.ok("schemaVersion=1"),
        Some(version) => report.error(format!("schemaVersion must be 1, got {version}")),
        None => report.error("schemaVersion is missing or not an integer"),
    }

    let players = parsed
        .get("replay")
        .and_then(|replay| replay.get("players"))
        .and_then(Value::as_array);
    let teams = parsed
        .get("replay")
        .and_then(|replay| replay.get("teams"))
        .and_then(Value::as_array);
    let mut player_slots = HashSet::new();

    if let Some(players) = players {
        report.ok(format!("players={}", players.len()));
        for player in players {
            match value_i32(player.get("slotId")) {
                Some(slot_id) if player_slots.insert(slot_id) => {}
                Some(slot_id) => report.error(format!("duplicate player slotId {slot_id}")),
                None => report.warning("player without integer slotId"),
            }
        }
    } else {
        report.error("replay.players is missing or not an array");
    }

    let team_member_slots = team_member_slot_ids(parsed);
    let mut actor_slots = player_slots.clone();
    actor_slots.extend(team_member_slots);

    if let Some(teams) = teams {
        report.ok(format!("teams={}", teams.len()));
        validate_team_members(teams, &actor_slots, &mut report);
    } else {
        report.error("replay.teams is missing or not an array");
    }

    let Some(events) = timeline_events(parsed) else {
        report.error("timeline.events is missing or not an array");
        return report;
    };

    let mut ids = HashSet::new();
    let mut missing_id_count = 0usize;
    let mut duplicate_id_count = 0usize;
    let mut unsorted_count = 0usize;
    let mut invalid_time_count = 0usize;
    let mut invalid_actor_model_count = 0usize;
    let mut invalid_player_actor_count = 0usize;
    let mut unknown_actor_count = 0usize;
    let mut invalid_payload_count = 0usize;
    let mut unknown_type_count = 0usize;
    let mut invalid_shipment_actor_count = 0usize;
    let mut chat_count = 0usize;
    let mut resign_count = 0usize;
    let mut shipment_count = 0usize;
    let mut shipment_confirmed_count = 0usize;
    let mut shipment_candidate_count = 0usize;
    let mut previous_time = None;

    for event in events {
        match event.get("id").and_then(Value::as_str) {
            Some(id) if id.is_empty() => missing_id_count += 1,
            Some(id) if ids.insert(id.to_string()) => {}
            Some(_) => duplicate_id_count += 1,
            None => missing_id_count += 1,
        }

        let time = value_i32(event.get("timeMs")).or_else(|| value_i32(event.get("time")));
        match time {
            Some(time) if time >= 0 => {
                if previous_time.is_some_and(|previous| time < previous) {
                    unsorted_count += 1;
                }
                previous_time = Some(time);
            }
            Some(_) | None => invalid_time_count += 1,
        }

        let event_type = event.get("type").and_then(Value::as_str);
        match event_type {
            Some("chat") => chat_count += 1,
            Some("resign") => resign_count += 1,
            Some("shipment") => {
                shipment_count += 1;
                match event
                    .get("payload")
                    .and_then(|payload| payload.get("status"))
                    .and_then(Value::as_str)
                {
                    Some("confirmed") => shipment_confirmed_count += 1,
                    Some("candidate") | None => shipment_candidate_count += 1,
                    Some(_) => {}
                }
            }
            // Verified command-derived gameplay events (count toward eventCount).
            Some("research") | Some("train") | Some("build") | Some("age_up") => {}
            Some(_) | None => unknown_type_count += 1,
        }

        if event_type == Some("shipment")
            && event
                .get("actor")
                .and_then(|actor| actor.get("kind"))
                .and_then(Value::as_str)
                != Some("player")
        {
            invalid_shipment_actor_count += 1;
        }

        match validate_actor(event.get("actor"), &actor_slots) {
            ActorValidation::Valid => {}
            ActorValidation::Unknown => unknown_actor_count += 1,
            ActorValidation::InvalidModel => invalid_actor_model_count += 1,
            ActorValidation::InvalidPlayer => invalid_player_actor_count += 1,
        }

        if !payload_matches_type(event, event_type) {
            invalid_payload_count += 1;
        }
    }

    report.ok(format!("events={}", events.len()));
    report.ok(format!("chat={chat_count}"));
    report.ok(format!("resign={resign_count}"));
    report.ok(format!("shipment={shipment_count}"));
    report.ok(format!("shipmentConfirmed={shipment_confirmed_count}"));
    report.ok(format!("shipmentCandidate={shipment_candidate_count}"));

    if missing_id_count == 0 && duplicate_id_count == 0 {
        report.ok("ids unique");
    } else {
        if missing_id_count > 0 {
            report.error(format!("{missing_id_count} event(s) have missing ids"));
        }
        if duplicate_id_count > 0 {
            report.error(format!("{duplicate_id_count} duplicate event id(s)"));
        }
    }

    if unsorted_count == 0 && invalid_time_count == 0 {
        report.ok("events sorted by time");
    } else {
        if unsorted_count > 0 {
            report.error(format!("{unsorted_count} event(s) are out of time order"));
        }
        if invalid_time_count > 0 {
            report.error(format!("{invalid_time_count} event(s) have invalid time"));
        }
    }

    if invalid_actor_model_count == 0 && invalid_player_actor_count == 0 {
        report.ok("timeline events have valid actor model");
    } else {
        if invalid_actor_model_count > 0 {
            report.error(format!(
                "{invalid_actor_model_count} event(s) have invalid actor model"
            ));
        }
        if invalid_player_actor_count > 0 {
            report.error(format!(
                "{invalid_player_actor_count} player actor(s) do not resolve to players"
            ));
        }
    }

    if invalid_shipment_actor_count > 0 {
        report.error(format!(
            "{invalid_shipment_actor_count} shipment event(s) do not have player actors"
        ));
    }

    if unknown_actor_count > 0 {
        report.warning(format!("{unknown_actor_count} event(s) have unknown actor"));
    } else {
        report.ok("no unknown actors");
    }

    if invalid_payload_count == 0 {
        report.ok("payload kinds match event types");
    } else {
        report.error(format!(
            "{invalid_payload_count} event(s) have payload/type mismatch"
        ));
    }

    if unknown_type_count > 0 {
        report.warning(format!("{unknown_type_count} event(s) have unknown type"));
    }

    validate_summary(
        parsed,
        events.len(),
        chat_count,
        resign_count,
        shipment_count,
        shipment_confirmed_count,
        shipment_candidate_count,
        players.map_or(0, Vec::len),
        teams.map_or(0, Vec::len),
        &mut report,
    );
    validate_result(parsed, &mut report);
    validate_debug(parsed, &actor_slots, &mut report);

    report
}

enum ActorValidation {
    Valid,
    Unknown,
    InvalidModel,
    InvalidPlayer,
}

fn validate_actor(actor: Option<&Value>, player_slots: &HashSet<i32>) -> ActorValidation {
    let Some(actor) = actor else {
        return ActorValidation::InvalidModel;
    };

    match actor.get("kind").and_then(Value::as_str) {
        Some("player") => match value_i32(actor.get("slotId")) {
            Some(slot_id) if player_slots.contains(&slot_id) => ActorValidation::Valid,
            Some(_) | None => ActorValidation::InvalidPlayer,
        },
        Some("system") => match actor.get("slotId") {
            None | Some(Value::Null) => ActorValidation::Valid,
            Some(value) if value.as_i64() == Some(0) => ActorValidation::Valid,
            Some(_) => ActorValidation::InvalidModel,
        },
        Some("unknown") => ActorValidation::Unknown,
        Some(_) | None => ActorValidation::InvalidModel,
    }
}

fn validate_team_members(
    teams: &[Value],
    player_slots: &HashSet<i32>,
    report: &mut ValidationReport,
) {
    let mut unknown_member_count = 0usize;

    for team in teams {
        let Some(members) = team.get("members").and_then(Value::as_array) else {
            report.warning("team without members array");
            continue;
        };

        for member in members {
            match value_i32(Some(member)) {
                Some(slot_id) if player_slots.contains(&slot_id) => {}
                Some(_) | None => unknown_member_count += 1,
            }
        }
    }

    if unknown_member_count > 0 {
        report.warning(format!(
            "{unknown_member_count} team member slotId(s) do not resolve to players"
        ));
    }
}

fn payload_matches_type(event: &Value, event_type: Option<&str>) -> bool {
    let Some(payload) = event.get("payload") else {
        return false;
    };
    let kind = payload.get("kind").and_then(Value::as_str);

    match event_type {
        Some("chat") => {
            kind == Some("chat")
                && payload.get("message").and_then(Value::as_str).is_some()
                && value_i32(payload.get("toId")).is_some()
        }
        Some("resign") => kind == Some("resign"),
        Some("shipment") => {
            kind == Some("shipment")
                && value_i32(payload.get("rawCommandId")).is_some()
                && matches!(
                    payload.get("confidence").and_then(Value::as_str),
                    Some("low" | "medium" | "high")
                )
                && matches!(
                    payload.get("status").and_then(Value::as_str),
                    Some("confirmed" | "candidate")
                )
                && payload.get("source").and_then(Value::as_str).is_some()
        }
        Some("research") | Some("age_up") => {
            kind == event_type
                && value_i32(payload.get("techId")).is_some()
                && payload.get("name").and_then(Value::as_str).is_some()
        }
        Some("train") => {
            kind == Some("train")
                && value_i32(payload.get("unitId")).is_some()
                && payload.get("name").and_then(Value::as_str).is_some()
        }
        Some("build") => {
            kind == Some("build")
                && value_i32(payload.get("buildingId")).is_some()
                && payload.get("name").and_then(Value::as_str).is_some()
        }
        _ => payload.is_object(),
    }
}

fn validate_summary(
    parsed: &Value,
    event_count: usize,
    chat_count: usize,
    resign_count: usize,
    shipment_count: usize,
    shipment_confirmed_count: usize,
    shipment_candidate_count: usize,
    player_count: usize,
    team_count: usize,
    report: &mut ValidationReport,
) {
    let Some(summary) = parsed.get("summary").and_then(Value::as_object) else {
        report.warning("summary is missing");
        return;
    };

    let expected = [
        ("eventCount", event_count),
        ("chatCount", chat_count),
        ("resignCount", resign_count),
        ("shipmentCount", shipment_count),
        ("shipmentConfirmedCount", shipment_confirmed_count),
        ("shipmentCandidateCount", shipment_candidate_count),
        ("playerCount", player_count),
        ("teamCount", team_count),
    ];
    let mut mismatch_count = 0usize;

    for (key, expected_value) in expected {
        match summary.get(key).and_then(Value::as_u64) {
            Some(value) if value as usize == expected_value => {}
            Some(value) => {
                mismatch_count += 1;
                report.error(format!(
                    "summary.{key} expected {expected_value}, got {value}"
                ));
            }
            None => {
                mismatch_count += 1;
                report.error(format!("summary.{key} is missing or not an integer"));
            }
        }
    }

    if mismatch_count == 0 {
        report.ok("summary counts match timeline and replay");
    }
}

fn validate_result(parsed: &Value, report: &mut ValidationReport) {
    let Some(result) = parsed.get("result").and_then(Value::as_object) else {
        report.error("result is missing or not an object");
        return;
    };

    let mut error_count = 0usize;

    if result.get("inferred").and_then(Value::as_bool).is_none() {
        error_count += 1;
        report.error("result.inferred is missing or not a boolean");
    }

    match result.get("confidence").and_then(Value::as_str) {
        Some("low" | "medium" | "high") => {}
        Some(value) => {
            error_count += 1;
            report.error(format!("result.confidence has unsupported value '{value}'"));
        }
        None => {
            error_count += 1;
            report.error("result.confidence is missing or not a string");
        }
    }

    for key in ["winningTeams", "losingTeams"] {
        match result.get(key).and_then(Value::as_array) {
            Some(values) if values.iter().all(|value| value_i32(Some(value)).is_some()) => {}
            Some(_) => {
                error_count += 1;
                report.error(format!("result.{key} must contain integer team ids"));
            }
            None => {
                error_count += 1;
                report.error(format!("result.{key} is missing or not an array"));
            }
        }
    }

    if result.get("reason").and_then(Value::as_str).is_none() {
        error_count += 1;
        report.error("result.reason is missing or not a string");
    }

    if error_count == 0 {
        report.ok("result shape valid");
    }
}

fn validate_debug(parsed: &Value, player_slots: &HashSet<i32>, report: &mut ValidationReport) {
    let Some(debug) = parsed.get("debug") else {
        return;
    };
    let Some(debug) = debug.as_object() else {
        report.error("debug is present but not an object");
        return;
    };
    let Some(commands) = debug.get("commands").and_then(Value::as_array) else {
        report.error("debug.commands is missing or not an array");
        return;
    };

    let mut invalid_command_count = 0usize;
    let mut unknown_counts: HashMap<String, usize> = HashMap::new();
    let mut command_counts: HashMap<String, usize> = HashMap::new();
    let mut shipment_candidate_count = 0usize;

    for command in commands {
        if command.get("offset").and_then(Value::as_u64).is_none()
            || command.get("timeMs").and_then(Value::as_i64).is_none()
            || command.get("commandId").and_then(Value::as_i64).is_none()
            || command.get("commandName").and_then(Value::as_str).is_none()
            || command.get("decoded").and_then(Value::as_bool).is_none()
            || command.get("length").and_then(Value::as_u64).is_none()
            || command.get("hexPreview").and_then(Value::as_str).is_none()
            || command.get("parsedAs").and_then(Value::as_str).is_none()
            || !debug_fields_valid(command.get("decodedFields"))
            || !raw_fields_valid(command.get("rawFields"))
        {
            invalid_command_count += 1;
        }

        match validate_actor(command.get("actor"), player_slots) {
            ActorValidation::Valid | ActorValidation::Unknown => {}
            ActorValidation::InvalidModel | ActorValidation::InvalidPlayer => {
                invalid_command_count += 1;
            }
        }

        if let Some(command_id) = command.get("commandId").and_then(Value::as_i64) {
            let key = command_id.to_string();
            *command_counts.entry(key.clone()).or_insert(0) += 1;
            if command
                .get("parsedAs")
                .and_then(Value::as_str)
                .is_some_and(is_unknown_parsed_as)
            {
                *unknown_counts.entry(key).or_insert(0) += 1;
            }
        }

        // "shipment_candidate" is the legacy name kept for older debug JSONs.
        if matches!(
            command.get("parsedAs").and_then(Value::as_str),
            Some("card_send_candidate" | "shipment_candidate")
        ) {
            shipment_candidate_count += 1;
        }
    }

    if invalid_command_count == 0 {
        report.ok(format!("debug commands={}", commands.len()));
    } else {
        report.error(format!(
            "{invalid_command_count} debug command(s) have invalid shape"
        ));
    }

    validate_debug_summary_counts(
        debug
            .get("debugSummary")
            .and_then(|summary| summary.get("commandIds")),
        &command_counts,
        "debug.debugSummary.commandIds",
        report,
    );
    validate_debug_summary_counts(
        debug
            .get("debugSummary")
            .and_then(|summary| summary.get("unknownCommandIds")),
        &unknown_counts,
        "debug.debugSummary.unknownCommandIds",
        report,
    );

    match debug
        .get("debugSummary")
        .and_then(|summary| summary.get("shipmentCandidateCount"))
        .and_then(Value::as_u64)
    {
        Some(count) if count as usize == shipment_candidate_count => {}
        Some(count) => report.error(format!(
            "debug.debugSummary.shipmentCandidateCount expected {shipment_candidate_count}, got {count}"
        )),
        None => report.error("debug.debugSummary.shipmentCandidateCount is missing or not an integer"),
    }
}

fn debug_fields_valid(value: Option<&Value>) -> bool {
    value.and_then(Value::as_object).is_some_and(|fields| {
        fields
            .values()
            .all(|value| value_i32(Some(value)).is_some())
    })
}

fn raw_fields_valid(value: Option<&Value>) -> bool {
    let Some(fields) = value.and_then(Value::as_object) else {
        return false;
    };
    ["u16le", "u32le"].iter().all(|key| {
        fields
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().all(raw_field_item_valid))
    })
}

fn raw_field_item_valid(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.get("offset").and_then(Value::as_u64).is_none() {
        return false;
    }

    object.get("value").and_then(Value::as_u64).is_some()
        || (object.get("u32").and_then(Value::as_u64).is_some()
            && object.get("i32").and_then(Value::as_i64).is_some())
}

fn validate_debug_summary_counts(
    value: Option<&Value>,
    expected: &HashMap<String, usize>,
    label: &str,
    report: &mut ValidationReport,
) {
    let Some(object) = value.and_then(Value::as_object) else {
        report.error(format!("{label} is missing or not an object"));
        return;
    };

    if object.len() != expected.len() {
        report.error(format!(
            "{label} has {} entries, expected {}",
            object.len(),
            expected.len()
        ));
        return;
    }

    for (key, expected_count) in expected {
        match object.get(key).and_then(Value::as_u64) {
            Some(count) if count as usize == *expected_count => {}
            Some(count) => report.error(format!(
                "{label}.{key} expected {expected_count}, got {count}"
            )),
            None => report.error(format!("{label}.{key} is missing")),
        }
    }
}

fn timeline_events(parsed: &Value) -> Option<&Vec<Value>> {
    parsed
        .get("timeline")
        .and_then(|timeline| timeline.get("events"))
        .and_then(Value::as_array)
}

fn value_i32(value: Option<&Value>) -> Option<i32> {
    value?.as_i64().and_then(|value| i32::try_from(value).ok())
}

fn is_unknown_parsed_as(parsed_as: &str) -> bool {
    parsed_as.starts_with("unknown")
}

fn default_output_path(command_name: &str, input_path: &Path) -> PathBuf {
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("replay");

    match command_name {
        "normalize" => PathBuf::from(format!("{stem}.normalized.json")),
        _ => PathBuf::from(format!("{stem}.parsed.json")),
    }
}

fn usage() -> String {
    "Usage:\n  aoe3de-replay-rust parse <path-to-age3Yrec> [-o <output-json-path>] [--debug-commands] [--experimental-shipments] [--no-events]\n  aoe3de-replay-rust normalize <path-to-parsed-json> [-o <output-json-path>]\n  aoe3de-replay-rust validate <path-to-normalized-json>\n  aoe3de-replay-rust inspect-commands <path-to-debug-json> [--from <timeMs>] [--to <timeMs>] [--command-id <id>] [--actor-slot <slot>] [--parsed-as <label>] [--limit <n>] [--full-hex]\n  aoe3de-replay-rust inspect-card-commands <path-to-debug-json> [--actor-slot <slot>]\n  aoe3de-replay-rust compare-commands --a <debug-json> --a-offset <offset> --b <debug-json> --b-offset <offset> [--limit <n>] [--show-same]\n  aoe3de-replay-rust compare-summaries --a <debug-json> --b <debug-json>\n  aoe3de-replay-rust dump-decks <path-to-json> [--slot <slotId>] [--card-id <rawId>]\n  aoe3de-replay-rust player-summary <path-to-debug-json>\n  aoe3de-replay-rust resolve-card --card-id <id>\n  aoe3de-replay-rust resolve-unit --unit-id <id>\n  aoe3de-replay-rust resolve-tech --tech-id <id>\n  aoe3de-replay-rust resolve-building --building-id <id>\n  aoe3de-replay-rust import-aoe3-companion --input <aoe3-companion path> [--out <data dir>]\n  aoe3de-replay-rust validate-corpus <dir-of-age3Yrec>\n  aoe3de-replay-rust capture --offsets <offsets-json> [--hz <n>] [--duration <seconds>] [-o <output-json-path>]\n  aoe3de-replay-rust merge-capture --replay <parsed-json> --capture <capture-json> [--offset-ms <ms>] [-o <output-json-path>]".to_string()
}

#[derive(Default)]
struct ValidationReport {
    ok: Vec<String>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl ValidationReport {
    fn ok(&mut self, message: impl Into<String>) {
        self.ok.push(message.into());
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    fn print(&self) {
        for message in &self.ok {
            println!("OK {message}");
        }
        for message in &self.warnings {
            println!("WARN {message}");
        }
        for message in &self.errors {
            println!("ERR {message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_accepts_minimal_normalized_json() {
        let report = validate_parsed_json(&sample_normalized_json());

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn validation_errors_when_timeline_is_unsorted() {
        let mut parsed = sample_normalized_json();
        parsed["timeline"]["events"][1]["time"] = json!(1);

        let report = validate_parsed_json(&parsed);

        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("out of time order")));
    }

    #[test]
    fn validation_warns_on_unknown_actor() {
        let mut parsed = sample_normalized_json();
        parsed["timeline"]["events"][0]["actor"] = unknown_actor_json(Some(99));

        let report = validate_parsed_json(&parsed);

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown actor")));
    }

    #[test]
    fn validation_accepts_system_actor() {
        let mut parsed = sample_normalized_json();
        parsed["timeline"]["events"][0]["actor"] = system_actor_json();

        let report = validate_parsed_json(&parsed);

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn validation_accepts_debug_commands() {
        let mut parsed = sample_normalized_json();
        parsed["debug"] = json!({
            "commands": [
                {
                    "offset": 100,
                    "timeMs": 5,
                    "actor": { "kind": "player", "slotId": 1, "playerId": 1, "name": "One" },
                    "commandId": 37,
                    "commandName": "command_37_unclassified",
                    "decoded": false,
                    "length": 48,
                    "hexPreview": "0a 00 12 ff",
                    "parsedAs": "unknown",
                    "decodedFields": {
                        "selectedCount": 0
                    },
                    "rawFields": {
                        "u16le": [
                            { "offset": 0, "value": 10 }
                        ],
                        "u32le": [
                            { "offset": 0, "u32": 10, "i32": 10 }
                        ]
                    }
                }
            ],
            "debugSummary": {
                "commandIds": { "37": 1 },
                "unknownCommandIds": { "37": 1 },
                "shipmentCandidateCount": 0
            }
        });

        let report = validate_parsed_json(&parsed);

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn normalize_adds_summary_to_legacy_commands() {
        let parsed = json!({
            "replay": {
                "players": [
                    { "slotId": 1, "playerName": "One" },
                    { "slotId": 2, "playerName": "Two" }
                ],
                "teams": []
            },
            "commands": {
                "chat": [
                    { "fromId": 1, "toId": 2, "message": "gg", "time": 5 }
                ],
                "resigns": [
                    { "slotId": 2, "time": 10 }
                ]
            }
        });

        let normalized = normalize_parsed_json(parsed).unwrap();

        assert_eq!(normalized["schemaVersion"], json!(1));
        assert_eq!(normalized["summary"]["eventCount"], json!(2));
        assert_eq!(normalized["summary"]["chatCount"], json!(1));
        assert_eq!(normalized["summary"]["resignCount"], json!(1));
        assert_eq!(normalized["summary"]["shipmentCount"], json!(0));
        assert_eq!(normalized["summary"]["shipmentConfirmedCount"], json!(0));
        assert_eq!(normalized["summary"]["shipmentCandidateCount"], json!(0));
        assert_eq!(
            normalized["timeline"]["events"][0]["actor"]["kind"],
            json!("player")
        );
        assert!(normalized.get("result").is_some());
        assert!(normalized.get("commands").is_none());
    }

    fn sample_normalized_json() -> Value {
        json!({
            "schemaVersion": 1,
            "timeline": {
                "events": [
                    {
                        "id": "event-000001",
                        "type": "chat",
                        "time": 2,
                        "actor": { "kind": "player", "slotId": 1, "playerId": 1, "name": "One" },
                        "payload": { "kind": "chat", "toId": 2, "message": "hi" }
                    },
                    {
                        "id": "event-000002",
                        "type": "resign",
                        "time": 12,
                        "actor": { "kind": "player", "slotId": 2, "playerId": 2, "name": "Two" },
                        "payload": { "kind": "resign" }
                    }
                ]
            },
            "summary": {
                "eventCount": 2,
                "chatCount": 1,
                "resignCount": 1,
                "shipmentCount": 0,
                "shipmentConfirmedCount": 0,
                "shipmentCandidateCount": 0,
                "playerCount": 2,
                "teamCount": 1
            },
            "result": {
                "inferred": false,
                "confidence": "low",
                "winningTeams": [],
                "losingTeams": [],
                "reason": "Could not infer winner without at least two valid teams"
            },
            "replay": {
                "players": [
                    { "slotId": 1, "playerName": "One" },
                    { "slotId": 2, "playerName": "Two" }
                ],
                "teams": [
                    { "id": 1, "name": "Team One", "members": [1, 2] }
                ]
            }
        })
    }
}
