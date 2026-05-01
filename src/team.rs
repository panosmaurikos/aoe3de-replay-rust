use crate::binary::{read_i32, read_utf16_string, search};
use crate::models::Team;

pub fn parse_teams(data: &[u8]) -> Vec<Team> {
    let mut teams = Vec::new();
    let mut position = 0usize;

    loop {
        position = match search(data, &[0x54, 0x45], position) {
            Some(value) => value,
            None => break,
        };

        position += 6;
        let key = match read_i32(data, position) {
            Ok(value) => value,
            Err(_) => break,
        };
        position += 4;

        if key == 12 {
            let team_id = match read_i32(data, position) {
                Ok(value) => value,
                Err(_) => break,
            };
            position += 4;

            let string_length = match read_i32(data, position) {
                Ok(value) if value >= 0 => value as usize,
                _ => break,
            };
            position += 4;

            let team_name = match read_utf16_string(data, position, string_length) {
                Ok(value) => value,
                Err(_) => break,
            };
            position += string_length * 2;

            let team_members_count = match read_i32(data, position) {
                Ok(value) => value.max(0),
                Err(_) => break,
            };
            position += 4;

            let mut members = Vec::new();
            for _ in 0..team_members_count {
                match read_i32(data, position) {
                    Ok(value) => members.push(value),
                    Err(_) => break,
                }
                position += 4;
            }

            teams.push(Team {
                id: team_id,
                name: team_name,
                members,
            });
        }
    }

    teams
}
