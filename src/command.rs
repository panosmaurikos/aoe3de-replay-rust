use crate::binary::{
    advance, decompress_replay, read_i32, read_i32_advance, read_u8, read_u8_advance,
    read_utf16_string, search,
};
use crate::models::{
    CardSendCandidate, Commands, DebugRawFields, DebugU16Field, DebugU32Field, Message,
    RawDebugCommand, Resign,
};
use crate::ParseResult;
use std::collections::BTreeMap;

pub fn parse_command(file_bytes: &[u8]) -> ParseResult<Commands> {
    Ok(parse_command_internal(file_bytes, false)?.commands)
}

pub fn parse_command_debug(file_bytes: &[u8]) -> ParseResult<(Commands, Vec<RawDebugCommand>)> {
    let parsed = parse_command_internal(file_bytes, true)?;
    Ok((parsed.commands, parsed.debug_commands))
}

struct ParsedCommands {
    commands: Commands,
    debug_commands: Vec<RawDebugCommand>,
}

fn parse_command_internal(file_bytes: &[u8], collect_debug: bool) -> ParseResult<ParsedCommands> {
    let data = decompress_replay(file_bytes)?;
    let mut chat = Vec::new();
    let mut resigns = Vec::new();
    let mut card_sends = Vec::new();
    let mut debug_commands = Vec::new();

    let start_marker = [0x9a, 0x99, 0x99, 0x3d];
    let mut position = search(&data, &start_marker, 0)
        .ok_or_else(|| "Could not find starting message marker".to_string())?
        + 142;

    let msg_len = read_i32_advance(&data, &mut position)?;
    for _ in 0..msg_len.max(0) {
        chat.push(read_message(&data, &mut position, 0)?);
    }

    let mut duration = 0i32;
    let command_marker = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x19];

    while let Some(found) = search(&data, &command_marker, position) {
        position = found;
        advance(&mut position, 113, data.len())?;
        let main_command = read_u8_advance(&data, &mut position)?;

        let Some((extra_skip, has_commands)) = main_command_layout(main_command) else {
            eprintln!("Unknown main command: {main_command}");
            continue;
        };

        advance(&mut position, extra_skip, data.len())?;

        let message_count = read_i32_advance(&data, &mut position)?;
        for _ in 0..message_count.max(0) {
            chat.push(read_message(&data, &mut position, duration)?);
        }

        duration += read_u8_advance(&data, &mut position)? as i32;

        if has_commands {
            let commands_count = if command_count_is_i32(main_command) {
                read_i32_advance(&data, &mut position)?
            } else {
                read_u8_advance(&data, &mut position)? as i32
            };

            for _ in 0..commands_count.max(0) {
                let debug_command = parse_inner_command(
                    &data,
                    &mut position,
                    duration,
                    &mut resigns,
                    &mut card_sends,
                )?;
                if collect_debug {
                    debug_commands.push(debug_command);
                }
            }
        }
    }

    Ok(ParsedCommands {
        commands: Commands {
            chat,
            resigns,
            card_sends,
        },
        debug_commands,
    })
}

fn read_message(data: &[u8], position: &mut usize, time: i32) -> ParseResult<Message> {
    let from_id = read_i32_advance(data, position)?;
    let to_id = read_i32_advance(data, position)?;
    let buf_len = read_i32_advance(data, position)?;
    if buf_len < 0 {
        return Err(format!(
            "Negative message length at byte offset {}",
            *position
        ));
    }

    let message = read_utf16_string(data, *position, buf_len as usize)?;
    advance(position, buf_len as usize * 2, data.len())?;
    advance(position, 1, data.len())?;

    Ok(Message {
        from_id,
        to_id,
        message,
        time,
    })
}

fn main_command_layout(command: u8) -> Option<(usize, bool)> {
    match command {
        33 | 65 | 161 | 193 => Some((0, true)),
        1 | 129 => Some((0, false)),
        35 | 37 | 41 | 67 | 73 | 163 | 165 | 169 | 195 | 201 => Some((4, true)),
        3 | 5 | 9 | 131 | 133 | 137 => Some((4, false)),
        39 | 43 | 45 | 75 | 167 | 171 | 173 | 203 => Some((8, true)),
        7 | 11 | 13 | 135 | 139 | 141 => Some((8, false)),
        47 | 175 | 207 => Some((12, true)),
        15 | 143 => Some((12, false)),
        49 | 177 => Some((36, true)),
        17 | 145 => Some((36, false)),
        19 | 21 | 25 | 147 | 149 | 153 => Some((40, false)),
        51 | 53 | 57 | 179 | 181 | 185 => Some((40, true)),
        55 | 59 | 61 | 183 | 187 | 189 => Some((44, true)),
        23 | 27 | 29 | 151 | 155 | 157 => Some((44, false)),
        63 | 191 | 223 => Some((48, true)),
        31 | 159 => Some((48, false)),
        _ => None,
    }
}

fn command_count_is_i32(command: u8) -> bool {
    matches!(
        command,
        65 | 67 | 73 | 75 | 193 | 195 | 201 | 203 | 207 | 223
    )
}

fn parse_inner_command(
    data: &[u8],
    position: &mut usize,
    duration: i32,
    resigns: &mut Vec<Resign>,
    card_sends: &mut Vec<CardSendCandidate>,
) -> ParseResult<RawDebugCommand> {
    let command_start = *position;
    read_u8_advance(data, position)?;
    let command_id = read_i32_advance(data, position)?;
    let mut decoded_fields = BTreeMap::new();

    let mut _shipment_cancel = -1;
    if command_id == 14 {
        let a = read_i32(data, *position)?;
        let b = read_i32(data, *position + 4)?;
        let c = read_i32(data, *position + 8)?;
        decoded_fields.insert("cancelA".to_string(), a);
        decoded_fields.insert("cancelB".to_string(), b);
        decoded_fields.insert("cancelC".to_string(), c);
        if a != -1 {
            _shipment_cancel = b;
        }
        advance(position, 12, data.len())?;
    }

    read_u8_advance(data, position)?;
    let player_slot_id = read_i32_advance(data, position)?;

    read_i32_advance(data, position)?;
    read_i32_advance(data, position)?;
    read_i32_advance(data, position)?;

    let unknown0 = read_i32_advance(data, position)?;
    if unknown0 == 1 {
        read_i32_advance(data, position)?;
    } else if unknown0 != 0 {
        eprintln!("unknown");
    }

    let mut unknown1 = read_i32_advance(data, position)?;
    let selected_count = read_i32_advance(data, position)?;
    decoded_fields.insert("unknown1".to_string(), unknown1);
    decoded_fields.insert("selectedCount".to_string(), selected_count);
    for _ in 0..selected_count.max(0) {
        read_i32_advance(data, position)?;
    }

    let mut unknown2 = read_i32_advance(data, position)?;
    if unknown2 < 0 {
        return Err(format!(
            "Negative variable block count at byte offset {}",
            *position
        ));
    }
    decoded_fields.insert("variableBlockCount".to_string(), unknown2);
    advance(position, unknown2 as usize * 12, data.len())?;

    let unknown_count = read_i32_advance(data, position)?;
    decoded_fields.insert("unknownByteCount".to_string(), unknown_count);
    for _ in 0..unknown_count.max(0) {
        read_u8_advance(data, position)?;
    }

    read_u8_advance(data, position)?;
    read_i32_advance(data, position)?;
    read_i32_advance(data, position)?;
    read_i32_advance(data, position)?;
    read_i32_advance(data, position)?;
    advance(position, 4, data.len())?;

    let mut parsed_as = parsed_as_for_command_id(command_id);
    let mut card_send: Option<CardSendCandidate> = None;
    match command_id {
        0 => {
            advance(position, 24, data.len())?;
            if read_u8(data, *position)? == 255 {
                advance(position, 8, data.len())?;
            }
        }
        1 => {
            let tech_id = read_i32_advance(data, position)?;
            decoded_fields.insert("techIdCandidate".to_string(), tech_id);
        }
        2 => {
            // Two observed variants share one layout:
            //   train unit:  protoId >= 0, deckIndex == -1 (Settler/villager spam clicks)
            //   card send:   protoId == -1, deckIndex >= 0 (index into the actor's ACTIVE deck)
            let proto_id = read_i32(data, *position)?;
            let deck_index = read_i32(data, *position + 4)?;
            decoded_fields.insert("unitProtoIdCandidate".to_string(), proto_id);
            decoded_fields.insert("deckIndexCandidate".to_string(), deck_index);
            if proto_id == -1 && deck_index >= 0 {
                parsed_as = "card_send_candidate";
                card_send = Some(CardSendCandidate {
                    slot_id: player_slot_id,
                    time: duration,
                    raw_command_id: command_id,
                    deck_index,
                });
            } else if proto_id >= 0 && deck_index == -1 {
                parsed_as = "train_unit_candidate";
            } else {
                parsed_as = "command2_unclassified";
            }
            if unknown1 == 0 || unknown1 == 2 {
                advance(position, 2, data.len())?;
            }
            advance(position, 14, data.len())?;
        }
        3 => {
            let proto_id = read_i32(data, *position)?;
            decoded_fields.insert("protoIdCandidate".to_string(), proto_id);
            advance(position, 44, data.len())?;
        }
        4 => advance(position, 25, data.len())?,
        6 => advance(position, 36, data.len())?,
        7 => advance(position, 1, data.len())?,
        9 => {}
        12 => {
            advance(position, 36, data.len())?;
            if unknown1 == 0 {
                advance(position, 1, data.len())?;
            }
        }
        13 => advance(position, 12, data.len())?,
        14 => {}
        16 => {
            advance(position, 4, data.len())?;
            let resign_slot_id = read_i32_advance(data, position)?;
            decoded_fields.insert("resignSlotId".to_string(), resign_slot_id);
            resigns.push(Resign {
                slot_id: resign_slot_id,
                time: duration,
            });
            advance(position, 5, data.len())?;
        }
        18 => advance(position, 4, data.len())?,
        19 => advance(position, 17, data.len())?,
        23 => advance(position, 6, data.len())?,
        24 => advance(position, 12, data.len())?,
        25 => advance(position, 6, data.len())?,
        26 => advance(position, 4, data.len())?,
        34 => {}
        35 => advance(position, 4, data.len())?,
        37 => advance(position, 5, data.len())?,
        41 => {
            let control1 = read_i32_advance(data, position)?;
            decoded_fields.insert("control1".to_string(), control1);
            advance(position, 4, data.len())?;
            advance(position, 4, data.len())?;
            advance(position, 8, data.len())?;
            unknown1 = read_i32_advance(data, position)?;
            decoded_fields.insert("command41Unknown1".to_string(), unknown1);
            let _ = unknown1;
            if control1 == 1 {
                unknown2 = read_i32_advance(data, position)?;
                decoded_fields.insert("command41Unknown2".to_string(), unknown2);
                if unknown2 == 1 {
                    let unknown3 = read_i32_advance(data, position)?;
                    decoded_fields.insert("command41Unknown3".to_string(), unknown3);
                }
                advance(position, 13, data.len())?;
            }
        }
        44 => advance(position, 8, data.len())?,
        46 => advance(position, 8, data.len())?,
        48 => advance(position, 9, data.len())?,
        53 => advance(position, 8, data.len())?,
        57 => advance(position, 12, data.len())?,
        58 => advance(position, 4, data.len())?,
        61 => advance(position, 8, data.len())?,
        62 => advance(position, 4, data.len())?,
        63 => advance(position, 16, data.len())?,
        64 => {}
        65 => advance(position, 4, data.len())?,
        66 => {
            // Two observed variants share one layout:
            //   deck select:   cardId == -1, deckId = the deck made active for this slot
            //   deck card add: cardId >= 0, appends cardId to deck deckId (in-game deck editing)
            let deck_id = read_i32_advance(data, position)?;
            let card_id = read_i32_advance(data, position)?;
            decoded_fields.insert("deckIdCandidate".to_string(), deck_id);
            decoded_fields.insert("cardIdCandidate".to_string(), card_id);
            parsed_as = if card_id == -1 {
                "deck_select_candidate"
            } else {
                "deck_card_add_candidate"
            };
        }
        67 => advance(position, 12, data.len())?,
        71 => advance(position, 4, data.len())?,
        72 => advance(position, 16, data.len())?,
        73 => {}
        80 => advance(position, 8, data.len())?,
        _ => parsed_as = "unknown_command_id",
    }

    let length = position.saturating_sub(command_start);
    if let Some(card_send) = card_send {
        card_sends.push(card_send);
    }
    let raw_fields = raw_fields(data, command_start, length, 128);

    Ok(RawDebugCommand {
        offset: command_start,
        time_ms: duration,
        player_slot_id,
        command_id,
        command_name: command_name_for_command_id(command_id).to_string(),
        decoded: command_is_decoded(command_id),
        length,
        hex_preview: hex_preview(data, command_start, length, 32),
        parsed_as: parsed_as.to_string(),
        decoded_fields,
        raw_fields,
    })
}

fn command_name_for_command_id(command_id: i32) -> &'static str {
    match command_id {
        0 => "unit_order_or_action",
        1 => "research_tech_candidate",
        2 => "train_or_card_send",
        3 => "proto_action_candidate",
        14 => "shipment_cancel_candidate",
        16 => "resign",
        37 => "command_37_unclassified",
        66 => "deck_select_or_card_add",
        _ => "known_layout_unclassified",
    }
}

fn command_is_decoded(command_id: i32) -> bool {
    matches!(command_id, 16)
}

fn parsed_as_for_command_id(command_id: i32) -> &'static str {
    match command_id {
        0 => "order",
        1 => "research_tech_candidate",
        2 => "command2_unclassified",
        3 => "proto_action_candidate",
        14 => "shipment_cancel_candidate",
        16 => "resign",
        37 => "command_37_unclassified",
        _ => "known_layout",
    }
}

fn raw_fields(data: &[u8], start: usize, length: usize, max_bytes: usize) -> DebugRawFields {
    let scan_len = length.min(max_bytes);
    let end = start.saturating_add(scan_len).min(data.len());
    let bytes = data.get(start..end).unwrap_or_default();

    let u16le = (0..bytes.len().saturating_sub(1))
        .step_by(2)
        .map(|offset| DebugU16Field {
            offset,
            value: u16::from_le_bytes([bytes[offset], bytes[offset + 1]]),
        })
        .collect();
    let u32le = (0..bytes.len().saturating_sub(3))
        .step_by(4)
        .map(|offset| {
            let raw = [
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ];
            DebugU32Field {
                offset,
                value_u32: u32::from_le_bytes(raw),
                value_i32: i32::from_le_bytes(raw),
            }
        })
        .collect();

    DebugRawFields { u16le, u32le }
}

fn hex_preview(data: &[u8], start: usize, length: usize, max_bytes: usize) -> String {
    let preview_len = length.min(max_bytes);
    let end = start.saturating_add(preview_len).min(data.len());
    let mut parts = data
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>();
    if length > max_bytes {
        parts.push("...".to_string());
    }
    parts.join(" ")
}
