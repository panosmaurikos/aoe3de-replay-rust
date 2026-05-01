use std::collections::HashMap;

use crate::binary::{read_bool, read_f32, read_i32, read_utf16_string};

#[derive(Clone, Debug)]
pub enum FieldValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
}

pub fn parse_fields(data: &[u8]) -> HashMap<String, FieldValue> {
    let mut dictionary = HashMap::new();
    let mut position = 0usize;
    let end_position = 20_000.min(data.len());
    let mut skip_count = 0usize;

    while position < end_position {
        let word = read_utf16_string(data, position, 1).unwrap_or_default();
        if is_english_word(&word) {
            if skip_count >= 4 {
                let next_word_length =
                    read_i32(data, position.saturating_sub(4)).unwrap_or_default();
                let mut is_next_word = next_word_length >= 0;
                let next_word = if is_next_word {
                    read_utf16_string(data, position, next_word_length as usize).unwrap_or_else(
                        |_| {
                            is_next_word = false;
                            String::new()
                        },
                    )
                } else {
                    String::new()
                };

                if is_next_word && next_word.chars().all(is_english_char) {
                    position = position.saturating_add(next_word.len() * 2);
                    let data_type = match read_i32(data, position) {
                        Ok(value) => value,
                        Err(_) => break,
                    };
                    position += 4;

                    match data_type {
                        1 => {
                            if let Ok(value) = read_f32(data, position) {
                                position += 4;
                                dictionary.insert(next_word, FieldValue::Float(value));
                            }
                        }
                        2 => {
                            if let Ok(value) = read_i32(data, position) {
                                position += 4;
                                dictionary.insert(next_word, FieldValue::Int(value));
                            }
                        }
                        5 => {
                            if let Ok(value) = read_bool(data, position) {
                                position += 1;
                                dictionary.insert(next_word, FieldValue::Bool(value));
                            }
                        }
                        9 => {
                            let string_length = match read_i32(data, position) {
                                Ok(value) => value,
                                Err(_) => break,
                            };
                            if string_length > 100 || string_length < 0 {
                                position = position.saturating_sub(4);
                            } else {
                                position += 4;
                                if let Ok(value) =
                                    read_utf16_string(data, position, string_length as usize)
                                {
                                    position += string_length as usize * 2;
                                    dictionary.insert(next_word, FieldValue::String(value));
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    position += 2;
                }
                skip_count = 0;
            } else {
                position += 2;
            }
        } else {
            position += 1;
            skip_count += 1;
        }
    }

    dictionary
}

pub fn get_string(dictionary: &HashMap<String, FieldValue>, key: &str) -> Option<String> {
    match dictionary.get(key) {
        Some(FieldValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

pub fn get_i32(dictionary: &HashMap<String, FieldValue>, key: &str) -> Option<i32> {
    match dictionary.get(key) {
        Some(FieldValue::Int(value)) => Some(*value),
        Some(FieldValue::Float(value)) => Some(*value as i32),
        _ => None,
    }
}

pub fn get_bool(dictionary: &HashMap<String, FieldValue>, key: &str) -> Option<bool> {
    match dictionary.get(key) {
        Some(FieldValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn is_english_word(value: &str) -> bool {
    value.chars().all(is_english_char)
}

fn is_english_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}
