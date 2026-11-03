use std::{
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use flate2::read::GzDecoder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{IoContext, Result, invalid};

const LIMIT: u64 = 1024 * 1024 + 1;
const BUNDLED: &[u8] = include_bytes!("../assets/event/rooms.csv.gz");

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Room {
    pub campus: String,
    pub building: String,
    pub room: String,
    pub kind: String,
    pub seats: u32,
    #[serde(default)]
    pub tour: String,
    #[serde(default)]
    pub photo: String,
}

impl Room {
    pub fn identity(&self) -> String {
        format!("{}|{}", self.campus, self.room)
    }
}

pub fn read(path: Option<&Path>) -> Result<Vec<Room>> {
    let mut raw = Vec::new();
    match path {
        Some(path) if path.extension().is_some_and(|suffix| suffix == "gz") => {
            let file = File::open(path).at_path("Cannot open", path)?;
            GzDecoder::new(file)
                .take(LIMIT)
                .read_to_end(&mut raw)
                .at_path("Cannot decompress", path)?;
        }
        Some(path) => {
            File::open(path)
                .at_path("Cannot open", path)?
                .take(LIMIT)
                .read_to_end(&mut raw)
                .at_path("Cannot read", path)?;
        }
        None => {
            GzDecoder::new(Cursor::new(BUNDLED))
                .take(LIMIT)
                .read_to_end(&mut raw)
                .at("Cannot decompress the bundled room catalogue")?;
        }
    }
    if raw.len() >= LIMIT as usize {
        return Err(invalid("Room CSV exceeds 1 MiB"));
    }
    let mut rooms = Vec::new();
    let mut seen = BTreeSet::new();
    let mut reader = csv::Reader::from_reader(Cursor::new(raw));
    for record in reader.deserialize::<Room>() {
        let mut room = record?;
        room.campus = room.campus.trim().to_owned();
        room.building = room.building.trim().to_owned();
        room.room = room.room.trim().to_owned();
        room.kind = room.kind.trim().to_owned();
        room.tour = room.tour.trim().to_owned();
        room.photo = room.photo.trim().to_owned();
        let identity = (room.campus.clone(), room.room.clone());
        if room.seats == 0
            || [&room.campus, &room.building, &room.room, &room.kind]
                .into_iter()
                .any(String::is_empty)
            || [&room.campus, &room.building, &room.room, &room.kind]
                .into_iter()
                .flat_map(|value| value.chars())
                .any(char::is_control)
            || !seen.insert(identity)
        {
            return Err(invalid(format!(
                "Invalid or duplicate room: {} {}",
                room.campus, room.room
            )));
        }
        validate_link(
            &room.tour,
            &["photos.google.com", "photos.app.goo.gl", "tour.panoee.net"],
        )?;
        validate_link(&room.photo, &["campuses.uq.edu.au"])?;
        rooms.push(room);
    }
    if rooms.is_empty() {
        return Err(invalid("Room CSV is empty"));
    }
    Ok(rooms)
}

pub fn search(rooms: &[Room], query: &str, minimum: u32) -> Result<Vec<Room>> {
    if query.chars().count() > 200 {
        return Err(invalid("Keep the room search under 200 characters"));
    }
    let capacity = Regex::new(r"(?i)\b(\d+)\s*(?:seats?|people|persons?|guests?)\b")
        .map_err(|error| invalid(error.to_string()))?;
    let mentioned = capacity
        .captures(query)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
        .unwrap_or_default();
    let cleaned = capacity.replace_all(query, " ");
    let terms = cleaned
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let minimum = minimum.max(mentioned);
    Ok(rooms
        .iter()
        .filter(|room| room.seats >= minimum)
        .filter(|room| {
            let haystack = format!(
                "{} {} {} {}",
                room.campus, room.building, room.room, room.kind
            )
            .to_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .take(20)
        .cloned()
        .collect())
}

fn validate_link(value: &str, hosts: &[&str]) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let url = Url::parse(value).map_err(|_| invalid("Invalid room-preview link"))?;
    if url.scheme() != "https"
        || url.port().is_some_and(|port| port != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.host_str().is_some_and(|host| hosts.contains(&host))
    {
        return Err(invalid("Invalid room-preview link"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalogue_is_valid_and_searchable() {
        let rooms = read(None).expect("bundled catalogue");
        assert!(rooms.len() > 100);
        let matches = search(&rooms, "Steele 40 people", 0).expect("room search");
        assert!(matches.iter().all(|room| room.seats >= 40));
        assert!(
            matches
                .iter()
                .all(|room| room.building.to_lowercase().contains("steele"))
        );
    }

    #[test]
    fn rejects_unsafe_links() {
        assert!(validate_link("javascript:alert(1)", &["campuses.uq.edu.au"]).is_err());
        assert!(
            validate_link(
                "https://campuses.uq.edu.au@evil.test/photo",
                &["campuses.uq.edu.au"]
            )
            .is_err()
        );
    }
}
