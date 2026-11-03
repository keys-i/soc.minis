use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use chrono::{NaiveDate, NaiveTime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::error::{IoContext, Result, invalid};

#[derive(Clone, Debug, Serialize)]
pub struct Coordinator {
    pub name: String,
    pub role: String,
    pub email: String,
    pub phone: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Schedule {
    pub setup_start: String,
    pub setup_end: String,
    pub event_start: String,
    pub event_end: String,
    pub pack_up_start: String,
    pub pack_up_end: String,
}

impl Schedule {
    pub fn compact(&self) -> String {
        [
            ("setup", &self.setup_start, &self.setup_end),
            ("event", &self.event_start, &self.event_end),
            ("pack-up", &self.pack_up_start, &self.pack_up_end),
        ]
        .into_iter()
        .filter(|(_, start, end)| !start.is_empty() || !end.is_empty())
        .map(|(label, start, end)| format!("{label} {}", join_range(start, end)))
        .collect::<Vec<_>>()
        .join("; ")
    }

    pub fn require_for_form(&self) -> Result<()> {
        let missing = [
            ("setup_start", &self.setup_start),
            ("setup_end", &self.setup_end),
            ("event_start", &self.event_start),
            ("event_end", &self.event_end),
            ("pack_up_start", &self.pack_up_start),
            ("pack_up_end", &self.pack_up_end),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.is_empty().then_some(format!("[schedule].{name}")))
        .collect::<Vec<_>>();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(invalid(format!(
                "Microsoft Form output requires {}",
                missing.join(", ")
            )))
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RiskOverride {
    pub aspect: Option<String>,
    pub hazards: Option<Vec<String>>,
    pub initial_risk: Option<String>,
    pub controls: Option<String>,
    pub residual_risk: Option<String>,
    pub responsible: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    #[serde(skip)]
    pub source: PathBuf,
    pub output_name: String,
    pub output_mode: OutputMode,
    pub risk_layout: RiskLayout,
    pub club_name: String,
    pub club_short_name: String,
    pub executives: String,
    pub event_officer: String,
    pub title: String,
    pub date_iso: String,
    pub date_display: String,
    pub campus: String,
    pub building: String,
    pub room: String,
    pub location: String,
    pub category: String,
    pub summary: String,
    pub risk_scope: String,
    pub target_audience: String,
    pub estimated_attendance: String,
    pub stakeholders: Vec<String>,
    pub activity: String,
    pub entertainment: String,
    pub additional_notes: String,
    pub coordinator: Coordinator,
    pub schedule: Schedule,
    pub indoor: bool,
    pub seated: bool,
    pub vehicles: bool,
    pub temporary_structures: bool,
    pub electrical_equipment: bool,
    pub cash_handling: bool,
    pub alcohol: bool,
    pub food_provided: bool,
    pub physical_competitions: bool,
    pub off_site_impact: bool,
    pub equipment_description: String,
    pub food_description: String,
    pub payment_method: String,
    pub vehicle_controls: String,
    pub temporary_structure_controls: String,
    pub weather_controls: String,
    pub competition_controls: String,
    pub alcohol_controls: String,
    pub application_title: String,
    pub application_fields: Vec<(String, String)>,
    pub form_action: String,
    pub form_started_at: Option<String>,
    pub form_submitted_at: Option<String>,
    pub form_values: BTreeMap<String, String>,
    pub answer_overrides: BTreeMap<String, String>,
    pub risk_overrides: BTreeMap<u8, RiskOverride>,
}

impl Event {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).at_path("Cannot read", path)?;
        let raw = text.parse::<Table>()?;
        Self::from_table(path, &raw)
    }

    pub fn from_table(path: &Path, raw: &Table) -> Result<Self> {
        let club = section(raw, "club")?;
        let event = section(raw, "event")?;
        let coordinator = section(raw, "coordinator")?;
        let schedule = optional_section(raw, "schedule")?;
        let catering = optional_section(raw, "catering")?;
        let operations = optional_section(raw, "operations")?;
        let document = optional_section(raw, "document")?;
        let output = optional_section(raw, "output")?;
        let application = optional_section(raw, "application")?;
        let form = optional_section(raw, "form")?;

        let club_name = required(club, "name", "club")?;
        let club_short_name =
            text(club.get("short_name"), "[club].short_name")?.unwrap_or_else(|| club_name.clone());
        let executives = text(club.get("executives"), "[club].executives")?
            .unwrap_or_else(|| format!("{club_name} Executives"));
        let event_officer = text(club.get("event_officer"), "[club].event_officer")?
            .unwrap_or_else(|| format!("{club_name} Events Officer"));
        let title = required(event, "title", "event")?;
        let (date_iso, date_display) = parse_date(event.get("date"))?;
        let campus = text(event.get("campus"), "[event].campus")?.unwrap_or_default();
        let building = text(event.get("building"), "[event].building")?.unwrap_or_default();
        let room = text(event.get("room"), "[event].room")?.unwrap_or_default();
        let location = text(event.get("location"), "[event].location")?
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| location(&campus, &building, &room));
        if location.is_empty() {
            return Err(invalid(
                "Provide [event].location or at least one of room, building and campus",
            ));
        }
        let summary = text(event.get("summary"), "[event].summary")?
            .filter(|value| !value.is_empty())
            .or(text(event.get("description"), "[event].description")?)
            .ok_or_else(|| invalid("Missing required [event].summary"))?;
        let estimated_attendance = required(event, "estimated_attendance", "event")?;
        let coordinator_name = required(coordinator, "name", "coordinator")?;
        let schedule = Schedule {
            setup_start: parse_time(schedule.get("setup_start"), "[schedule].setup_start")?,
            setup_end: parse_time(schedule.get("setup_end"), "[schedule].setup_end")?,
            event_start: parse_time(schedule.get("event_start"), "[schedule].event_start")?,
            event_end: parse_time(schedule.get("event_end"), "[schedule].event_end")?,
            pack_up_start: parse_time(schedule.get("pack_up_start"), "[schedule].pack_up_start")?,
            pack_up_end: parse_time(schedule.get("pack_up_end"), "[schedule].pack_up_end")?,
        };
        let indoor = boolean(operations, "indoor", true)?;
        let seated = boolean(operations, "seated", true)?;
        let food_description =
            text(catering.get("description"), "[catering].description")?.unwrap_or_default();
        let output_name = text(document.get("output_name"), "[document].output_name")?
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("{date_iso}-{}", slug(&title)));
        let output_name_pattern = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,119}$")
            .map_err(|error| invalid(error.to_string()))?;
        if !output_name_pattern.is_match(&output_name) {
            return Err(invalid(
                "[document].output_name must be 1–120 letters, numbers, dots, underscores or hyphens",
            ));
        }
        let application_fields = application
            .get("fields")
            .map(|value| {
                value
                    .as_table()
                    .ok_or_else(|| invalid("[application.fields] must be a TOML table"))?
                    .iter()
                    .map(|(key, value)| {
                        Ok((
                            label(key),
                            scalar(value, &format!("[application.fields].{key}"))?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let form_values = string_table(form.get("values"), "[form.values]")?;
        let answer_overrides =
            string_table(form.get("answer_overrides"), "[form.answer_overrides]")?;
        let risk_overrides = parse_risk_overrides(raw.get("risk_overrides"))?;
        let activity = text(event.get("activity"), "[event].activity")?
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| title.clone());
        let entertainment = text(event.get("entertainment"), "[event].entertainment")?
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| activity.clone());

        Ok(Self {
            source: path.to_path_buf(),
            output_name,
            output_mode: parse_mode(text(output.get("mode"), "[output].mode")?.as_deref())?,
            risk_layout: parse_layout(
                text(output.get("risk_layout"), "[output].risk_layout")?.as_deref(),
            )?,
            club_name: club_name.clone(),
            club_short_name,
            executives: executives.clone(),
            event_officer,
            title,
            date_iso,
            date_display,
            campus,
            building,
            room,
            location,
            category: text(event.get("category"), "[event].category")?
                .unwrap_or_else(|| "Small".into()),
            summary,
            risk_scope: text(event.get("risk_scope"), "[event].risk_scope")?.unwrap_or_default(),
            target_audience: text(event.get("target_audience"), "[event].target_audience")?
                .unwrap_or_else(|| "UQ students".into()),
            estimated_attendance,
            stakeholders: strings(event.get("stakeholders"), "[event].stakeholders")?
                .unwrap_or_else(|| {
                    vec![
                        executives,
                        "event attendees".into(),
                        "UQ Union/Student Associations".into(),
                        "venue staff".into(),
                        "UQ Property and Facilities".into(),
                    ]
                }),
            activity,
            entertainment,
            additional_notes: text(event.get("additional_notes"), "[event].additional_notes")?
                .unwrap_or_default(),
            coordinator: Coordinator {
                name: coordinator_name,
                role: text(coordinator.get("role"), "[coordinator].role")?.unwrap_or_default(),
                email: text(coordinator.get("email"), "[coordinator].email")?.unwrap_or_default(),
                phone: text(coordinator.get("phone"), "[coordinator].phone")?.unwrap_or_default(),
            },
            schedule,
            indoor,
            seated,
            vehicles: boolean(operations, "vehicles", false)?,
            temporary_structures: boolean(operations, "temporary_structures", false)?,
            electrical_equipment: boolean(operations, "electrical_equipment", true)?,
            cash_handling: boolean(operations, "cash_handling", false)?,
            alcohol: boolean(operations, "alcohol", false)?,
            food_provided: boolean(catering, "provided", !food_description.is_empty())?,
            physical_competitions: boolean(operations, "physical_competitions", false)?,
            off_site_impact: boolean(operations, "off_site_impact", !indoor)?,
            equipment_description: text(operations.get("equipment"), "[operations].equipment")?
                .unwrap_or_else(|| "existing UQ AV equipment, laptops and chargers".into()),
            food_description,
            payment_method: text(
                operations.get("payment_method"),
                "[operations].payment_method",
            )?
            .unwrap_or_else(|| "approved cashless or pre-registration methods".into()),
            vehicle_controls: text(
                operations.get("vehicle_controls"),
                "[operations].vehicle_controls",
            )?
            .unwrap_or_default(),
            temporary_structure_controls: text(
                operations.get("temporary_structure_controls"),
                "[operations].temporary_structure_controls",
            )?
            .unwrap_or_default(),
            weather_controls: text(
                operations.get("weather_controls"),
                "[operations].weather_controls",
            )?
            .unwrap_or_default(),
            competition_controls: text(
                operations.get("competition_controls"),
                "[operations].competition_controls",
            )?
            .unwrap_or_default(),
            alcohol_controls: text(
                operations.get("alcohol_controls"),
                "[operations].alcohol_controls",
            )?
            .unwrap_or_default(),
            application_title: text(application.get("title"), "[application].title")?
                .unwrap_or_else(|| "EVENT APPLICATION SUMMARY".into()),
            application_fields,
            form_action: text(form.get("action"), "[form].action")?
                .unwrap_or_else(|| "preview".into()),
            form_started_at: text(form.get("started_at"), "[form].started_at")?,
            form_submitted_at: text(form.get("submitted_at"), "[form].submitted_at")?,
            form_values,
            answer_overrides,
            risk_overrides,
        })
    }

    pub fn context(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("club".into(), self.club_name.clone()),
            ("club_short".into(), self.club_short_name.clone()),
            ("executives".into(), self.executives.clone()),
            ("event_officer".into(), self.event_officer.clone()),
            ("event_title".into(), self.title.clone()),
            ("event_date".into(), self.date_display.clone()),
            ("event_date_iso".into(), self.date_iso.clone()),
            ("location".into(), self.location.clone()),
            ("category".into(), self.category.clone()),
            ("summary".into(), self.summary.clone()),
            ("coordinator".into(), self.coordinator.name.clone()),
            ("coordinator_role".into(), self.coordinator.role.clone()),
            ("coordinator_email".into(), self.coordinator.email.clone()),
            ("coordinator_phone".into(), self.coordinator.phone.clone()),
            ("target_audience".into(), self.target_audience.clone()),
            ("attendance".into(), self.estimated_attendance.clone()),
            ("activity".into(), self.activity.clone()),
            ("entertainment".into(), self.entertainment.clone()),
            ("equipment".into(), self.equipment_description.clone()),
            ("food".into(), self.food_description.clone()),
            ("payment_method".into(), self.payment_method.clone()),
            ("campus".into(), self.campus.clone()),
            ("room".into(), self.room.clone()),
            ("setup_start".into(), self.schedule.setup_start.clone()),
            ("setup_end".into(), self.schedule.setup_end.clone()),
            ("event_start".into(), self.schedule.event_start.clone()),
            ("event_end".into(), self.schedule.event_end.clone()),
            ("pack_up_start".into(), self.schedule.pack_up_start.clone()),
            ("pack_up_end".into(), self.schedule.pack_up_end.clone()),
        ])
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Form,
    Risk,
    Both,
}

impl OutputMode {
    pub const fn form(self) -> bool {
        matches!(self, Self::Form | Self::Both)
    }

    pub const fn risk(self) -> bool {
        matches!(self, Self::Risk | Self::Both)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum RiskLayout {
    Form,
    Pack,
    Both,
}

fn section<'a>(raw: &'a Table, name: &str) -> Result<&'a Table> {
    raw.get(name)
        .and_then(Value::as_table)
        .ok_or_else(|| invalid(format!("Missing required [{name}] table")))
}

fn optional_section<'a>(raw: &'a Table, name: &str) -> Result<&'a Table> {
    match raw.get(name) {
        Some(value) => value
            .as_table()
            .ok_or_else(|| invalid(format!("[{name}] must be a TOML table"))),
        None => Ok(empty()),
    }
}

fn empty() -> &'static Table {
    static TABLE: std::sync::OnceLock<Table> = std::sync::OnceLock::new();
    TABLE.get_or_init(Table::new)
}

fn required(table: &Table, key: &str, section: &str) -> Result<String> {
    text(table.get(key), &format!("[{section}].{key}"))?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("Missing required [{section}].{key}")))
}

fn scalar(value: &Value, field: &str) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.trim().to_owned()),
        Value::Integer(value) => Ok(value.to_string()),
        Value::Float(value) => Ok(value.to_string()),
        Value::Boolean(value) => Ok(if *value { "Yes" } else { "No" }.into()),
        Value::Datetime(value) => Ok(value.to_string()),
        Value::Array(values) => values
            .iter()
            .map(|item| scalar(item, field))
            .collect::<Result<Vec<_>>>()
            .map(|values| values.join(", ")),
        Value::Table(_) => Err(invalid(format!("{field} must be a scalar or array"))),
    }
}

fn text(value: Option<&Value>, field: &str) -> Result<Option<String>> {
    value.map(|value| scalar(value, field)).transpose()
}

fn strings(value: Option<&Value>, field: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let values = value
        .as_array()
        .ok_or_else(|| invalid(format!("{field} must be an array of strings")))?
        .iter()
        .map(|value| scalar(value, field))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(
        values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect(),
    ))
}

fn boolean(table: &Table, key: &str, default: bool) -> Result<bool> {
    match table.get(key) {
        None => Ok(default),
        Some(Value::Boolean(value)) => Ok(*value),
        Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "yes" | "true" | "1" | "y" => Ok(true),
            "no" | "false" | "0" | "n" => Ok(false),
            _ => Err(invalid(format!("{key} must be true or false"))),
        },
        Some(_) => Err(invalid(format!("{key} must be true or false"))),
    }
}

fn parse_date(value: Option<&Value>) -> Result<(String, String)> {
    let value = text(value, "[event].date")?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("Missing required [event].date"))?;
    for format in ["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&value, format) {
            return Ok((
                date.format("%Y-%m-%d").to_string(),
                date.format("%d/%m/%Y").to_string(),
            ));
        }
    }
    Err(invalid("[event].date must be YYYY-MM-DD or DD/MM/YYYY"))
}

fn parse_time(value: Option<&Value>, field: &str) -> Result<String> {
    let Some(value) = text(value, field)? else {
        return Ok(String::new());
    };
    if value.is_empty() {
        return Ok(value);
    }
    for format in ["%H:%M", "%H:%M:%S", "%I:%M %p"] {
        if let Ok(time) = NaiveTime::parse_from_str(&value, format) {
            return Ok(time.format("%H:%M").to_string());
        }
    }
    Err(invalid(format!("{field} must be a TOML time or HH:MM")))
}

fn location(campus: &str, building: &str, room: &str) -> String {
    let main = match (room.is_empty(), building.is_empty()) {
        (false, false) => format!("{room} ({building})"),
        (false, true) => room.to_owned(),
        (true, false) => building.to_owned(),
        (true, true) => String::new(),
    };
    match (main.is_empty(), campus.is_empty()) {
        (false, false) => format!("{main}, {campus}"),
        (false, true) => main,
        (true, false) => campus.to_owned(),
        (true, true) => String::new(),
    }
}

fn slug(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            result.push(character);
            separator = false;
        } else if !separator && !result.is_empty() {
            result.push('-');
            separator = true;
        }
    }
    result.trim_end_matches('-').to_owned()
}

fn label(value: &str) -> String {
    value
        .replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn string_table(value: Option<&Value>, field: &str) -> Result<BTreeMap<String, String>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    value
        .as_table()
        .ok_or_else(|| invalid(format!("{field} must be a TOML table")))?
        .iter()
        .map(|(key, value)| Ok((key.clone(), scalar(value, &format!("{field}.{key}"))?)))
        .collect()
}

fn parse_risk_overrides(value: Option<&Value>) -> Result<BTreeMap<u8, RiskOverride>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    value
        .as_table()
        .ok_or_else(|| invalid("[risk_overrides] must be a TOML table"))?
        .iter()
        .map(|(key, value)| {
            let number = key.parse::<u8>().map_err(|_| {
                invalid(format!(
                    "Risk override {key:?} must be a number from 1 to 22"
                ))
            })?;
            if !(1..=22).contains(&number) {
                return Err(invalid(format!(
                    "Risk override {number} must be between 1 and 22"
                )));
            }
            let values = value
                .as_table()
                .ok_or_else(|| invalid(format!("[risk_overrides.{key:?}] must be a TOML table")))?;
            let known = BTreeSet::from([
                "aspect",
                "hazards",
                "initial_risk",
                "controls",
                "residual_risk",
                "responsible",
            ]);
            if let Some(unknown) = values.keys().find(|key| !known.contains(key.as_str())) {
                return Err(invalid(format!("Unknown risk override field: {unknown}")));
            }
            Ok((
                number,
                RiskOverride {
                    aspect: text(values.get("aspect"), "risk aspect")?,
                    hazards: strings(values.get("hazards"), "risk hazards")?,
                    initial_risk: text(values.get("initial_risk"), "risk initial_risk")?,
                    controls: text(values.get("controls"), "risk controls")?,
                    residual_risk: text(values.get("residual_risk"), "risk residual_risk")?,
                    responsible: text(values.get("responsible"), "risk responsible")?,
                },
            ))
        })
        .collect()
}

fn join_range(start: &str, end: &str) -> String {
    match (start.is_empty(), end.is_empty()) {
        (false, false) => format!("{start}–{end}"),
        (false, true) => start.to_owned(),
        (true, false) => end.to_owned(),
        (true, true) => "not specified".into(),
    }
}

fn parse_mode(value: Option<&str>) -> Result<OutputMode> {
    match value.unwrap_or("both") {
        "form" => Ok(OutputMode::Form),
        "risk" => Ok(OutputMode::Risk),
        "both" => Ok(OutputMode::Both),
        _ => Err(invalid("[output].mode must be form, risk or both")),
    }
}

fn parse_layout(value: Option<&str>) -> Result<RiskLayout> {
    match value.unwrap_or("form") {
        "form" => Ok(RiskLayout::Form),
        "pack" => Ok(RiskLayout::Pack),
        "both" => Ok(RiskLayout::Both),
        _ => Err(invalid("[output].risk_layout must be form, pack or both")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[club]
name = "Film Society"

[event]
title = "Movie night"
date = 2026-11-20
campus = "St Lucia"
building = "Steele"
room = "03-309"
summary = "A film and snacks"
estimated_attendance = 40

[coordinator]
name = "Mina"
email = "mina@example.com"

[schedule]
event_start = 18:00:00
event_end = 20:00:00
"#;

    #[test]
    fn validates_event_and_builds_defaults() {
        let raw = SAMPLE.parse::<Table>().expect("sample TOML");
        let event = Event::from_table(Path::new("event.toml"), &raw).expect("valid event");
        assert_eq!(event.output_name, "2026-11-20-movie-night");
        assert_eq!(event.location, "03-309 (Steele), St Lucia");
        assert_eq!(event.schedule.compact(), "event 18:00–20:00");
    }

    #[test]
    fn rejects_unsafe_output_name() {
        let mut raw = SAMPLE.parse::<Table>().expect("sample TOML");
        raw.insert(
            "document".into(),
            Value::Table(Table::from_iter([(
                "output_name".into(),
                Value::String("../bad".into()),
            )])),
        );
        let error = Event::from_table(Path::new("event.toml"), &raw).expect_err("unsafe name");
        assert!(error.to_string().contains("output_name"));
    }
}
