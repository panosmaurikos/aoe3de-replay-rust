use std::collections::HashMap;

use crate::binary::{decompress_replay, read_i32, read_utf16_string};
use crate::constants::civ_info_for_replay_id;
use crate::deck::parse_decks;
use crate::fields::{get_bool, get_i32, get_string, parse_fields};
use crate::models::{GameSetting, MapInfo, Player, Replay};
use crate::team::parse_teams;
use crate::ParseResult;

pub fn parse_replay(file_bytes: &[u8]) -> ParseResult<Replay> {
    let data = decompress_replay(file_bytes)?;
    let exe_version = parse_exe_version(&data);
    let dictionary = parse_fields(&data);
    let map_infos = load_map_infos();
    let map_name = get_string(&dictionary, "gamefilename");
    let map_info = map_name
        .as_ref()
        .and_then(|name| map_infos.get(&name.to_lowercase()).cloned());

    let setting = GameSetting {
        game_name: get_string(&dictionary, "gamename"),
        allow_cheats: get_bool(&dictionary, "gameallowcheats"),
        blockade: get_bool(&dictionary, "gameblockade"),
        player_count: get_i32(&dictionary, "gamenumplayers"),
        difficulty: get_i32(&dictionary, "gamedifficulty"),
        starting_age: get_i32(&dictionary, "gamestartingage"),
        ending_age: get_i32(&dictionary, "gameendingage"),
        is_treaty: get_bool(&dictionary, "gamestartwithtreaty"),
        allow_trade_monopoly: get_bool(&dictionary, "gametrademonopoly"),
        game_type: get_i32(&dictionary, "gametype"),
        map_crc: get_i32(&dictionary, "gamefilecrc"),
        map_name: map_name.clone(),
        map_info,
        map_set: get_string(&dictionary, "gamefilenameext"),
        free_for_all: get_bool(&dictionary, "gamefreeforall"),
        host_time: get_i32(&dictionary, "gamehosttime"),
        koth: get_bool(&dictionary, "gamekoth"),
        latency: get_i32(&dictionary, "gamelatency"),
        map_set_name: get_string(&dictionary, "gamemapname"),
        map_resource: get_i32(&dictionary, "gamemapresources"),
        radom_seed: get_i32(&dictionary, "gamerandomseed"),
        game_speed: get_i32(&dictionary, "gamespeed"),
    };

    let player_count = setting.player_count.unwrap_or_default().max(0);
    let mut players = Vec::new();
    for index in 1..=player_count {
        let civ_id = get_i32(&dictionary, &format!("gameplayer{index}civ"));
        players.push(Player {
            ai_personality: get_string(&dictionary, &format!("gameplayer{index}aipersonality")),
            avatar_id: get_string(&dictionary, &format!("gameplayer{index}avatarid")),
            civ_id,
            civ_info: civ_id.and_then(civ_info_for_replay_id),
            civ_is_random: get_bool(&dictionary, &format!("gameplayer{index}civwasrandom")),
            clan: get_string(&dictionary, &format!("gameplayer{index}clan")),
            color: get_i32(&dictionary, &format!("gameplayer{index}color")),
            explorer_name: get_string(&dictionary, &format!("gameplayer{index}explorername")),
            explorer_skin_id: get_i32(&dictionary, &format!("gameplayer{index}explorerskinid")),
            handicap: get_i32(&dictionary, &format!("gameplayer{index}handicap")),
            homecity_file_name: get_string(&dictionary, &format!("gameplayer{index}hcfilename")),
            homecity_level: get_i32(&dictionary, &format!("gameplayer{index}hclevel")),
            homecity_name: get_string(&dictionary, &format!("gameplayer{index}homecityname")),
            slot_id: get_i32(&dictionary, &format!("gameplayer{index}id")),
            player_name: get_string(&dictionary, &format!("gameplayer{index}name")),
            initial_decks: Vec::new(),
        });
    }

    players.sort_by_key(|player| player.slot_id.unwrap_or(i32::MAX));
    assign_initial_decks(&mut players, parse_decks(&data));

    Ok(Replay {
        exe_version,
        setting,
        players,
        teams: parse_teams(&data),
    })
}

fn parse_exe_version(data: &[u8]) -> Option<i32> {
    let position = 273usize;
    let string_length = read_i32(data, position).ok()?;
    if string_length < 0 {
        return None;
    }

    let exe_info = read_utf16_string(data, position, string_length as usize).ok()?;
    exe_info
        .split(' ')
        .nth(1)
        .and_then(|value| value.parse::<i32>().ok())
}

fn assign_initial_decks(players: &mut [Player], all_decks: Vec<crate::models::Deck>) {
    if all_decks.is_empty() {
        return;
    }

    let mut deck_index = 0usize;
    for player in players {
        let mut initial_decks = Vec::new();
        let mut previous_deck_id = all_decks[0].deck_id;

        while deck_index < all_decks.len() {
            let deck = &all_decks[deck_index];
            if deck.deck_id < previous_deck_id || deck.deck_name == "*" {
                break;
            }

            initial_decks.push(deck.clone());
            previous_deck_id = deck.deck_id;
            deck_index += 1;
        }

        while deck_index < all_decks.len() {
            if all_decks[deck_index].deck_id == 0 {
                break;
            }
            deck_index += 1;
        }

        player.initial_decks = initial_decks;
    }
}

fn load_map_infos() -> HashMap<String, MapInfo> {
    serde_json::from_str(include_str!("maps.json")).unwrap_or_default()
}
