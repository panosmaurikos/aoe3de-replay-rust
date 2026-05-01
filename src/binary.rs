use flate2::read::DeflateDecoder;
use std::io::Read;

use crate::constants::HEADER_LENGTH;
use crate::ParseResult;

pub fn decompress_replay(file_bytes: &[u8]) -> ParseResult<Vec<u8>> {
    if file_bytes.len() < HEADER_LENGTH {
        return Err(format!(
            "Replay file is too short: expected at least {HEADER_LENGTH} bytes, got {}",
            file_bytes.len()
        ));
    }

    let mut decoder = DeflateDecoder::new(&file_bytes[HEADER_LENGTH..]);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|err| format!("Could not decompress replay body: {err}"))?;
    Ok(decompressed)
}

pub fn read_i32(data: &[u8], position: usize) -> ParseResult<i32> {
    let end = position
        .checked_add(4)
        .ok_or_else(|| "i32 byte offset overflow".to_string())?;
    let bytes = data
        .get(position..end)
        .ok_or_else(|| format!("Could not read i32 at byte offset {position}"))?;
    Ok(i32::from_le_bytes(
        bytes.try_into().expect("slice length checked"),
    ))
}

pub fn read_f32(data: &[u8], position: usize) -> ParseResult<f32> {
    let end = position
        .checked_add(4)
        .ok_or_else(|| "f32 byte offset overflow".to_string())?;
    let bytes = data
        .get(position..end)
        .ok_or_else(|| format!("Could not read f32 at byte offset {position}"))?;
    Ok(f32::from_le_bytes(
        bytes.try_into().expect("slice length checked"),
    ))
}

pub fn read_u8(data: &[u8], position: usize) -> ParseResult<u8> {
    data.get(position)
        .copied()
        .ok_or_else(|| format!("Could not read u8 at byte offset {position}"))
}

pub fn read_bool(data: &[u8], position: usize) -> ParseResult<bool> {
    Ok(read_u8(data, position)? != 0)
}

pub fn read_utf16_string(data: &[u8], position: usize, length: usize) -> ParseResult<String> {
    let byte_len = length
        .checked_mul(2)
        .ok_or_else(|| "UTF-16 string length overflow".to_string())?;
    let end = position
        .checked_add(byte_len)
        .ok_or_else(|| "UTF-16 byte offset overflow".to_string())?;
    let bytes = data
        .get(position..end)
        .ok_or_else(|| format!("Could not read UTF-16 string at byte offset {position}"))?;

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

pub fn search(data: &[u8], pattern: &[u8], from_index: usize) -> Option<usize> {
    if pattern.is_empty() {
        return Some(from_index.min(data.len()));
    }

    if from_index >= data.len() || pattern.len() > data.len() {
        return None;
    }

    data[from_index..]
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|offset| from_index + offset)
}

pub fn search_deck(data: &[u8], start_index: usize) -> Option<usize> {
    let mut position = start_index;
    while position < data.len() {
        position = search(data, &[0x00, 0x00, 0x00, 0x44, 0x6b], position)?;
        position += 9;
        if read_i32(data, position).ok()? == 5 {
            return Some(position - 4);
        }
    }
    None
}

pub fn advance(position: &mut usize, amount: usize, data_len: usize) -> ParseResult<()> {
    let next = position
        .checked_add(amount)
        .ok_or_else(|| "Position overflow while parsing replay".to_string())?;
    if next > data_len {
        return Err(format!(
            "Could not advance from byte offset {} by {amount} bytes; data length is {data_len}",
            *position
        ));
    }
    *position = next;
    Ok(())
}

pub fn read_i32_advance(data: &[u8], position: &mut usize) -> ParseResult<i32> {
    let value = read_i32(data, *position)?;
    *position += 4;
    Ok(value)
}

pub fn read_u8_advance(data: &[u8], position: &mut usize) -> ParseResult<u8> {
    let value = read_u8(data, *position)?;
    *position += 1;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_i32() {
        assert_eq!(read_i32(&[0x78, 0x56, 0x34, 0x12], 0).unwrap(), 0x12345678);
    }

    #[test]
    fn reads_bool_from_u8() {
        assert!(!read_bool(&[0], 0).unwrap());
        assert!(read_bool(&[2], 0).unwrap());
    }

    #[test]
    fn decodes_utf16_le_string() {
        let bytes = [b'A', 0, b'o', 0, b'E', 0];
        assert_eq!(read_utf16_string(&bytes, 0, 3).unwrap(), "AoE");
    }

    #[test]
    fn searches_byte_pattern() {
        assert_eq!(search(&[1, 2, 3, 2, 3], &[2, 3], 0), Some(1));
        assert_eq!(search(&[1, 2, 3, 2, 3], &[2, 3], 2), Some(3));
        assert_eq!(search(&[1, 2, 3], &[4], 0), None);
    }
}
