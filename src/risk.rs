use std::io::{Cursor, Write};

use serde::Serialize;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    error::{Result, invalid},
    model::{Event, RiskLayout},
};

#[derive(Clone, Debug, Serialize)]
pub struct RiskRow {
    pub number: u8,
    pub aspect: String,
    pub hazards: Vec<String>,
    pub initial_risk: String,
    pub controls: String,
    pub residual_risk: String,
    pub responsible: String,
}

struct Definition {
    aspect: &'static str,
    hazards: &'static [&'static str],
}

const DEFINITIONS: [Definition; 22] = [
    Definition {
        aspect: "Access / Egress to and from site",
        hazards: &[
            "Vehicle movements and reversing vehicles",
            "Attendee movement and safe exit",
            "Other",
        ],
    },
    Definition {
        aspect: "Set Up and Pack Up",
        hazards: &[
            "Slips, trips and falls",
            "Pinch or crush points",
            "Manual handling strains and sprains",
            "Incorrect equipment setup",
        ],
    },
    Definition {
        aspect: "Temporary Structures",
        hazards: &["Collapse", "Trip hazards", "Obstructed walkways or exits"],
    },
    Definition {
        aspect: "Adverse Weather",
        hazards: &["Cold or wet conditions", "High winds", "Heat exposure"],
    },
    Definition {
        aspect: "Electrically Powered Equipment",
        hazards: &[
            "Electric shock",
            "Trip hazards from cords",
            "Incorrect placement",
        ],
    },
    Definition {
        aspect: "Traffic Control",
        hazards: &["Vehicles on site before, during or after the event"],
    },
    Definition {
        aspect: "Number of Patrons (Crowd Control)",
        hazards: &[
            "Panic or protest",
            "Trampling or crush",
            "Excessive attendance",
            "Blocked exits",
        ],
    },
    Definition {
        aspect: "Waste Management",
        hazards: &[
            "Overflowing waste containers",
            "Spills or leftover materials",
        ],
    },
    Definition {
        aspect: "Provision of Amenities",
        hazards: &["Excessive queues", "Contamination", "Overloaded amenities"],
    },
    Definition {
        aspect: "Patron and Staff Wellness",
        hazards: &[
            "Heat exhaustion",
            "Dehydration",
            "Stress, anxiety or illness",
        ],
    },
    Definition {
        aspect: "Fire Safety",
        hazards: &["Fire on site", "Unsafe evacuation"],
    },
    Definition {
        aspect: "Provision of Entertainment",
        hazards: &["Excessive noise", "Unsafe or disruptive activity"],
    },
    Definition {
        aspect: "Cash Handling",
        hazards: &["Theft", "Hygiene", "Counting errors"],
    },
    Definition {
        aspect: "Security Matters",
        hazards: &[
            "Forced or unauthorised entry",
            "Disruptive behaviour",
            "Protection of attendees",
            "Drug use on site",
        ],
    },
    Definition {
        aspect: "Service of Alcohol",
        hazards: &[
            "Breach of licence",
            "Excessive intoxication",
            "Supply to minors",
            "Unwell patrons",
        ],
    },
    Definition {
        aspect: "Service of Food",
        hazards: &[
            "Food poisoning",
            "Allergens",
            "Spills and cross-contamination",
        ],
    },
    Definition {
        aspect: "Competitions",
        hazards: &["Injury during planned or impromptu competition"],
    },
    Definition {
        aspect: "On Site Incident Management",
        hazards: &[
            "Poor communication",
            "Alarm or protocol failure",
            "Delayed response",
        ],
    },
    Definition {
        aspect: "Consequential Outcomes",
        hazards: &["Sexual assault", "Conflict or fights", "Harassment"],
    },
    Definition {
        aspect: "Campus Assets",
        hazards: &[
            "Damage to flora and fauna",
            "Damage to buildings or equipment",
        ],
    },
    Definition {
        aspect: "Off Site Matters",
        hazards: &["Public complaints", "Noise", "Neighbour impacts"],
    },
    Definition {
        aspect: "Reputational Risks",
        hazards: &["Loss of reputation", "Misconduct", "Negative feedback"],
    },
];

pub fn rows(event: &Event) -> Result<Vec<RiskRow>> {
    DEFINITIONS
        .iter()
        .enumerate()
        .map(|(index, definition)| {
            let number = u8::try_from(index + 1).map_err(|error| invalid(error.to_string()))?;
            let (initial, controls, residual, responsible) = controls(number, event);
            let override_ = event.risk_overrides.get(&number);
            Ok(RiskRow {
                number,
                aspect: expand(
                    override_
                        .and_then(|value| value.aspect.as_deref())
                        .unwrap_or(definition.aspect),
                    event,
                )?,
                hazards: override_
                    .and_then(|value| value.hazards.clone())
                    .unwrap_or_else(|| definition.hazards.iter().map(ToString::to_string).collect())
                    .into_iter()
                    .map(|value| expand(&value, event))
                    .collect::<Result<Vec<_>>>()?,
                initial_risk: expand(
                    override_
                        .and_then(|value| value.initial_risk.as_deref())
                        .unwrap_or(initial),
                    event,
                )?,
                controls: expand(
                    override_
                        .and_then(|value| value.controls.as_deref())
                        .unwrap_or(&controls),
                    event,
                )?,
                residual_risk: expand(
                    override_
                        .and_then(|value| value.residual_risk.as_deref())
                        .unwrap_or(residual),
                    event,
                )?,
                responsible: expand(
                    override_
                        .and_then(|value| value.responsible.as_deref())
                        .unwrap_or(&responsible),
                    event,
                )?,
            })
        })
        .collect()
}

pub fn document(event: &Event, layout: RiskLayout) -> Result<Vec<u8>> {
    let risks = rows(event)?;
    let mut body = String::new();
    if matches!(layout, RiskLayout::Pack) {
        body.push_str(&heading(&event.application_title, 1));
        body.push_str(&table(&[
            vec!["Event".into(), event.title.clone()],
            vec!["Society".into(), event.club_name.clone()],
            vec!["Date".into(), event.date_display.clone()],
            vec!["Venue".into(), event.location.clone()],
            vec!["Attendance".into(), event.estimated_attendance.clone()],
            vec!["Coordinator".into(), coordinator(event)],
            vec!["Schedule".into(), event.schedule.compact()],
            vec!["Activity".into(), event.summary.clone()],
        ]));
        if !event.application_fields.is_empty() {
            body.push_str(&heading("Reviewed application details", 2));
            body.push_str(&table(
                &event
                    .application_fields
                    .iter()
                    .map(|(label, value)| vec![label.clone(), value.clone()])
                    .collect::<Vec<_>>(),
            ));
        }
        body.push_str(&page_break());
    }
    body.push_str(&heading("Event Risk Assessment", 1));
    body.push_str(&paragraph(&format!(
        "{} · {} · {} · {} expected attendees",
        event.title, event.date_display, event.location, event.estimated_attendance
    )));
    let description = if event.risk_scope.trim().is_empty() {
        &event.summary
    } else {
        &event.risk_scope
    };
    body.push_str(&paragraph(description));
    if !event.additional_notes.is_empty() {
        body.push_str(&paragraph(&event.additional_notes));
    }
    let mut risk_table = vec![vec![
        "#".into(),
        "Aspect & hazards".into(),
        "Initial".into(),
        "Controls".into(),
        "Residual".into(),
        "Responsible".into(),
    ]];
    risk_table.extend(risks.into_iter().map(|risk| {
        vec![
            risk.number.to_string(),
            format!("{}\n{}", risk.aspect, risk.hazards.join("\n")),
            risk.initial_risk,
            risk.controls,
            risk.residual_risk,
            risk.responsible,
        ]
    }));
    body.push_str(&table(&risk_table));
    make_docx(event, &body)
}

fn controls(number: u8, event: &Event) -> (&'static str, String, &'static str, String) {
    let executives = event.executives.clone();
    let officer = event.event_officer.clone();
    match number {
        1 => (
            if event.vehicles { "Medium" } else { "Low" },
            format!(
                "Use normal pedestrian access and marked exits at {}. Keep walkways, doors and emergency exits clear. {}",
                event.location,
                if event.vehicles {
                    fallback(&event.vehicle_controls, "Supervise vehicle movement and separate it from pedestrians")
                } else {
                    "No vehicles are expected".into()
                }
            ),
            "Low",
            executives,
        ),
        2 => (
            "Low-Medium",
            format!(
                "Retain the existing room layout where practical. Move furniture carefully and keep bags, boxes and cables out of walkways. {}",
                event.schedule.compact()
            ),
            "Low",
            executives,
        ),
        3 if !event.temporary_structures => na("No temporary structures, stages or barriers will be used"),
        3 => (
            "Medium",
            fallback(
                &event.temporary_structure_controls,
                "Competent people will install and check structures clear of exits and access ways",
            ),
            "Low",
            executives,
        ),
        4 if event.indoor => (
            "Low",
            "The event is indoors, limiting weather exposure. Attendees will use normal campus paths and buildings".into(),
            "Low",
            officer,
        ),
        4 => (
            "Medium",
            fallback(
                &event.weather_controls,
                "Check weather beforehand and keep shelter, water and a cancellation or relocation plan available",
            ),
            "Low",
            officer,
        ),
        5 if !event.electrical_equipment => na("No electrically powered equipment is planned"),
        5 => (
            "Low-Medium",
            format!(
                "Check {} for visible damage, route cables away from walkways and keep equipment clear of food and drinks",
                event.equipment_description
            ),
            "Low",
            officer,
        ),
        6 if !event.vehicles => na("No vehicle access, road closures, deliveries or traffic control are required"),
        6 => (
            "Medium",
            fallback(
                &event.vehicle_controls,
                "Use approved routes, schedule deliveries away from arrival periods and supervise vehicle access",
            ),
            "Low",
            officer,
        ),
        7 => (
            "Low-Medium",
            format!(
                "Keep attendance at or below the booked capacity of {}. Monitor arrivals and keep exits and aisles clear. The event will be {}",
                event.estimated_attendance,
                if event.seated { "seated and structured" } else { "managed by event staff" }
            ),
            "Low",
            executives,
        ),
        8 => (
            "Low",
            format!(
                "Use available bins and inspect the room after the event. {}",
                if event.food_provided {
                    format!("Dispose of waste from {} promptly", event.food_description)
                } else {
                    "Waste is expected to be limited".into()
                }
            ),
            "Low",
            executives,
        ),
        9 => (
            "Low",
            "Use the existing nearby venue amenities and monitor any unexpected queue or hygiene issue".into(),
            "Low",
            officer,
        ),
        10 => (
            "Low-Medium",
            format!(
                "Allow attendees to take breaks or leave. {} will monitor illness, distress or discomfort and contact UQ Security or emergency services when needed",
                event.executives
            ),
            "Low",
            executives,
        ),
        11 => (
            "Low-Medium",
            "Use the venue's alarms, marked exits and evacuation procedures. Keep exits clear and do not use flames or unapproved heat sources".into(),
            "Low",
            executives,
        ),
        12 => (
            "Low",
            format!(
                "Limit activities to {} and keep sound at a reasonable level. Stop unsafe or disruptive activity",
                event.entertainment
            ),
            "Low",
            officer,
        ),
        13 if !event.cash_handling => na(&format!(
            "No cash handling is expected; use {} if payment is needed",
            event.payment_method
        )),
        13 => (
            "Medium",
            format!(
                "Authorised {} officers will count cash in pairs and secure it promptly. Prefer {}",
                event.club_short_name, event.payment_method
            ),
            "Low",
            executives,
        ),
        14 => (
            "Low",
            format!(
                "{} will monitor attendance, address unsafe or unauthorised behaviour and contact UQ Security when needed",
                event.executives
            ),
            "Low",
            executives,
        ),
        15 if !event.alcohol => na("No alcohol will be served or permitted"),
        15 => (
            "High",
            fallback(
                &event.alcohol_controls,
                "Proceed only with required approvals, responsible service, age checks, water, food and support for unwell patrons",
            ),
            "Medium",
            executives,
        ),
        16 if !event.food_provided => na("No food will be prepared, served or distributed"),
        16 => (
            "Low-Medium",
            format!(
                "Handle {} safely, identify allergens where practical, keep food clear of electrical equipment and clean spills promptly",
                event.food_description
            ),
            "Low",
            executives,
        ),
        17 if !event.physical_competitions => (
            "Low",
            format!(
                "Activity is limited to {}. Do not permit physical, hazardous or impromptu competitions",
                event.activity
            ),
            "Low",
            officer,
        ),
        17 => (
            "Medium",
            fallback(
                &event.competition_controls,
                "Set rules and participant limits, clear the activity area and stop unsafe activity",
            ),
            "Low",
            officer,
        ),
        18 => (
            "Low-Medium",
            format!(
                "Report incidents to {} and follow UQ emergency procedures, contacting UQ Security or emergency services when required",
                event.coordinator.name
            ),
            "Low",
            executives,
        ),
        19 => (
            "Low-Medium",
            "Apply respectful behaviour expectations and escalate harassment, conflict, unwanted conduct or safety concerns to appropriate support".into(),
            "Low",
            executives,
        ),
        20 => (
            "Low",
            "Use furniture, fixtures and equipment appropriately, then inspect and return the venue to its original condition".into(),
            "Low",
            executives,
        ),
        21 if !event.off_site_impact => na("The event is contained within the booked venue and is unlikely to affect neighbouring areas"),
        21 => (
            "Low-Medium",
            "Monitor noise, attendee movement and access beyond the event area, and address complaints promptly".into(),
            "Low",
            officer,
        ),
        22 => (
            "Low",
            format!(
                "Run the event respectfully and in line with UQ and {} expectations. Address complaints, misconduct or negative feedback promptly",
                event.club_name
            ),
            "Low",
            executives,
        ),
        _ => na("Not applicable"),
    }
}

fn na(message: &str) -> (&'static str, String, &'static str, String) {
    ("N/A", message.into(), "N/A", "N/A".into())
}

fn fallback(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.into()
    } else {
        value.into()
    }
}

fn expand(value: &str, event: &Event) -> Result<String> {
    let context = event.context();
    let mut result = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail
            .find('}')
            .ok_or_else(|| invalid("Risk override has an unmatched opening brace"))?;
        let key = &tail[..end];
        result.push_str(
            context
                .get(key)
                .ok_or_else(|| invalid(format!("Unknown risk placeholder {{{key}}}")))?,
        );
        rest = &tail[end + 1..];
    }
    if rest.contains('}') {
        return Err(invalid("Risk override has an unmatched closing brace"));
    }
    result.push_str(rest);
    Ok(result)
}

fn make_docx(event: &Event, body: &str) -> Result<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    write_entry(
        &mut archive,
        "[Content_Types].xml",
        content_types(),
        options,
    )?;
    write_entry(&mut archive, "_rels/.rels", relationships(), options)?;
    write_entry(&mut archive, "docProps/core.xml", &core(event), options)?;
    write_entry(&mut archive, "docProps/app.xml", app(), options)?;
    write_entry(&mut archive, "word/styles.xml", styles(), options)?;
    write_entry(
        &mut archive,
        "word/_rels/document.xml.rels",
        document_rels(),
        options,
    )?;
    write_entry(
        &mut archive,
        "word/document.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="850" w:right="700" w:bottom="850" w:left="700" w:header="425" w:footer="425" w:gutter="0"/></w:sectPr></w:body></w:document>"#
        ),
        options,
    )?;
    Ok(archive.finish()?.into_inner())
}

fn write_entry(
    archive: &mut ZipWriter<Cursor<Vec<u8>>>,
    name: &str,
    value: &str,
    options: SimpleFileOptions,
) -> Result<()> {
    archive.start_file(name, options)?;
    archive
        .write_all(value.as_bytes())
        .map_err(|error| crate::error::SocError::Io {
            context: format!("Cannot write {name}"),
            source: error,
        })?;
    Ok(())
}

fn paragraph(value: &str) -> String {
    format!(
        r#"<w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        xml(value)
    )
}

fn heading(value: &str, level: u8) -> String {
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading{level}"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>"#,
        xml(value)
    )
}

fn page_break() -> String {
    r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#.into()
}

fn table(rows: &[Vec<String>]) -> String {
    let mut result = String::from(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="52606D"/><w:left w:val="single" w:sz="4" w:color="52606D"/><w:bottom w:val="single" w:sz="4" w:color="52606D"/><w:right w:val="single" w:sz="4" w:color="52606D"/><w:insideH w:val="single" w:sz="4" w:color="AAB4BE"/><w:insideV w:val="single" w:sz="4" w:color="AAB4BE"/></w:tblBorders></w:tblPr>"#,
    );
    for (index, row) in rows.iter().enumerate() {
        result.push_str("<w:tr>");
        for cell in row {
            let shade = if index == 0 && rows.first().is_some_and(|first| first.len() > 2) {
                r#"<w:shd w:fill="14213D"/><w:color w:val="FFFFFF"/>"#
            } else {
                ""
            };
            let lines = cell
                .split('\n')
                .map(|line| {
                    format!(
                        r#"<w:r><w:rPr>{shade}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
                        xml(line)
                    )
                })
                .collect::<Vec<_>>()
                .join(r"<w:r><w:br/></w:r>");
            result.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr><w:p>{lines}</w:p></w:tc>"#
            ));
        }
        result.push_str("</w:tr>");
    }
    result.push_str("</w:tbl>");
    result
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn coordinator(event: &Event) -> String {
    [
        event.coordinator.name.as_str(),
        event.coordinator.role.as_str(),
        event.coordinator.email.as_str(),
        event.coordinator.phone.as_str(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

fn content_types() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#
}

fn relationships() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#
}

fn document_rels() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
}

fn styles() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Aptos" w:hAnsi="Aptos"/><w:sz w:val="18"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="100" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:rPr><w:b/><w:color w:val="14213D"/><w:sz w:val="34"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:rPr><w:b/><w:color w:val="FF5A36"/><w:sz w:val="26"/></w:rPr></w:style></w:styles>"#
}

fn core(event: &Event) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:subject>Event risk assessment</dc:subject></cp:coreProperties>"#,
        xml(&event.title),
        xml(&event.club_name)
    )
}

fn app() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>SOC Minis</Application></Properties>"#
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use zip::ZipArchive;

    use super::*;

    #[test]
    fn produces_editable_word_documents() {
        let event = Event::load(Path::new("tests/movie-night.toml")).expect("sample event");
        let bytes = document(&event, RiskLayout::Pack).expect("risk pack");
        assert!(bytes.len() > 5_000);
        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("valid DOCX zip");
        let mut xml = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/document.xml").expect("document XML"),
            &mut xml,
        )
        .expect("read document XML");
        assert!(xml.contains("German Club Movie Night"));
        assert!(xml.contains("Event Risk Assessment"));
        assert!(xml.contains("Reputational Risks"));
        let _ = fs::metadata("assets/event/rooms.csv.gz").expect("asset is packaged");
    }
}
