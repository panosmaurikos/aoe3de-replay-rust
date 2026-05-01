use crate::binary::{read_bool, read_i32, read_utf16_string, search_deck};
use crate::models::{Deck, DeckCard};

pub fn parse_decks(data: &[u8]) -> Vec<Deck> {
    let mut decks = Vec::new();
    let mut position = 0usize;

    loop {
        let current_deck_position = position;
        let next_deck_offset = match read_i32(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 4;

        let check = match read_i32(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 4;

        if check != 5 {
            match search_deck(data, position) {
                Some(next_position) => {
                    position = next_position;
                    continue;
                }
                None => break,
            }
        }

        let deck_id = match read_i32(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 4;

        let string_length = match read_i32(data, position) {
            Ok(value) if value >= 0 => value as usize,
            _ => break,
        };
        position += 4;

        let deck_name = match read_utf16_string(data, position, string_length) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += string_length * 2;

        let game_id = match read_i32(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 4;

        let is_default = match read_bool(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 1;

        if read_bool(data, position).is_err() {
            break;
        }
        position += 1;

        let card_count = match read_i32(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 4;

        let mut tech_ids = Vec::new();
        for _ in 0..card_count.max(0) {
            match read_i32(data, position) {
                Ok(value) => tech_ids.push(value),
                Err(_) => break,
            }
            position += 4;
        }

        let cards = tech_ids
            .iter()
            .copied()
            .map(|raw_id| DeckCard { raw_id })
            .collect();

        decks.push(Deck {
            deck_name,
            deck_id,
            game_id,
            is_default,
            card_count,
            cards,
            tech_ids,
        });

        if next_deck_offset < 0 {
            break;
        }
        position = current_deck_position + next_deck_offset as usize + 6;
    }

    decks
}
