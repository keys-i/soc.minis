use std::{collections::BTreeMap, env};

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::json;
use toml::{Table, Value};
use url::Url;
use uuid::Uuid;

use crate::{
    error::{Result, invalid},
    model::Event,
};

const PROFILE: &str = include_str!("../assets/event/uq-event-booking.toml");
const REQUIRED_VALUES: [&str; 11] = [
    "affiliated",
    "date_flexibility",
    "under_18",
    "non_uq",
    "venue_capacity",
    "option_28",
    "food",
    "option_30",
    "option_31",
    "option_32",
    "terms",
];

#[derive(Clone, Debug)]
pub struct AnswerSpec {
    pub question_id: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct FormProfile {
    pub endpoint: String,
    pub referer: String,
    pub origin: String,
    pub ring: String,
    pub language: String,
    pub email_receipt_consent: bool,
    pub answers: Vec<AnswerSpec>,
    pub cookie_env: String,
    pub verification_token_env: String,
    pub session_id_env: String,
    pub muid_env: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Answer {
    #[serde(rename = "questionId")]
    pub question_id: String,
    #[serde(rename = "answer1")]
    pub answer: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormPayload {
    pub start_date: String,
    pub submit_date: String,
    pub answers: String,
    pub email_receipt_consent: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FormSecrets {
    pub cookie: String,
    pub verification_token: String,
    pub session_id: String,
    pub muid: String,
}

impl FormProfile {
    pub fn bundled() -> Result<Self> {
        let raw = PROFILE.parse::<Table>()?;
        Self::from_table(&raw)
    }

    pub fn from_table(raw: &Table) -> Result<Self> {
        let request = table(raw.get("request"), "[request]")?;
        let auth = table(raw.get("auth"), "[auth]")?;
        let endpoint = forms_url(
            required(request, "endpoint", "request")?,
            "[request].endpoint",
        )?;
        let referer = forms_url(
            required(request, "referer", "request")?,
            "[request].referer",
        )?;
        let origin = forms_url(
            text(request.get("origin"), "[request].origin")?
                .unwrap_or_else(|| "https://forms.cloud.microsoft".into()),
            "[request].origin",
        )?;
        let email_receipt_consent = request
            .get("email_receipt_consent")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answers = raw
            .get("answers")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("The form profile needs [[answers]] entries"))?;
        let mut seen = std::collections::BTreeSet::new();
        let answers = answers
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let item = value.as_table().ok_or_else(|| {
                    invalid(format!("[[answers]] entry {} must be a table", index + 1))
                })?;
                let question_id = required(item, "question_id", "answers")?;
                if !seen.insert(question_id.clone()) {
                    return Err(invalid(format!(
                        "Duplicate form question ID: {question_id}"
                    )));
                }
                Ok(AnswerSpec {
                    question_id,
                    value: text(item.get("value"), "answer value")?.unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if answers.is_empty() {
            return Err(invalid("The form profile needs at least one answer"));
        }
        Ok(Self {
            endpoint,
            referer,
            origin,
            ring: text(request.get("ring"), "[request].ring")?.unwrap_or_else(|| "business".into()),
            language: text(request.get("language"), "[request].language")?
                .unwrap_or_else(|| "en-AU,en;q=0.9".into()),
            email_receipt_consent,
            answers,
            cookie_env: env_name(auth, "cookie_env", "UQ_FORMS_COOKIE")?,
            verification_token_env: env_name(
                auth,
                "verification_token_env",
                "UQ_FORMS_VERIFICATION_TOKEN",
            )?,
            session_id_env: env_name(auth, "session_id_env", "UQ_FORMS_SESSION_ID")?,
            muid_env: env_name(auth, "muid_env", "UQ_FORMS_MUID")?,
        })
    }

    pub fn secrets(&self) -> Result<FormSecrets> {
        Ok(FormSecrets {
            cookie: secret(&self.cookie_env)?,
            verification_token: secret(&self.verification_token_env)?,
            session_id: secret(&self.session_id_env)?,
            muid: secret(&self.muid_env)?,
        })
    }
}

pub fn payload(event: &Event, profile: &FormProfile, now: DateTime<Utc>) -> Result<FormPayload> {
    event.schedule.require_for_form()?;
    if event.campus.is_empty() || event.room.is_empty() {
        return Err(invalid(
            "Microsoft Form output requires [event].campus and [event].room",
        ));
    }
    let missing_contact = [
        ("coordinator_role", &event.coordinator.role),
        ("coordinator_email", &event.coordinator.email),
        ("coordinator_phone", &event.coordinator.phone),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.is_empty().then_some(name))
    .collect::<Vec<_>>();
    if !missing_contact.is_empty() {
        return Err(invalid(format!(
            "Microsoft Form output requires {}",
            missing_contact.join(", ")
        )));
    }
    let missing_values = REQUIRED_VALUES
        .into_iter()
        .filter(|key| !event.form_values.contains_key(*key))
        .collect::<Vec<_>>();
    if !missing_values.is_empty() {
        return Err(invalid(format!(
            "Missing [form.values] keys: {}",
            missing_values.join(", ")
        )));
    }
    let mut context = event.context();
    context.extend(
        event
            .form_values
            .iter()
            .map(|(key, value)| (format!("form_{key}"), value.clone())),
    );
    let known = profile
        .answers
        .iter()
        .map(|answer| answer.question_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let unknown = event
        .answer_overrides
        .keys()
        .filter(|key| !known.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "Unknown form answer overrides: {}",
            unknown.join(", ")
        )));
    }
    let answers = profile
        .answers
        .iter()
        .map(|spec| {
            let template = event
                .answer_overrides
                .get(&spec.question_id)
                .unwrap_or(&spec.value);
            let answer = expand(template, &context)?;
            Ok(Answer {
                question_id: spec.question_id.clone(),
                answer: (!answer.is_empty()).then_some(answer),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let started = configured_time(event.form_started_at.as_deref())?.unwrap_or(now);
    let submitted = configured_time(event.form_submitted_at.as_deref())?.unwrap_or(now);
    if submitted < started {
        return Err(invalid(
            "[form].submitted_at cannot be earlier than [form].started_at",
        ));
    }
    Ok(FormPayload {
        start_date: timestamp(started),
        submit_date: timestamp(submitted),
        answers: serde_json::to_string(&answers)?,
        email_receipt_consent: profile.email_receipt_consent,
    })
}

pub async fn submit(
    profile: &FormProfile,
    payload: &FormPayload,
    secrets: &FormSecrets,
) -> Result<StatusCode> {
    for (name, value) in [
        ("cookie", &secrets.cookie),
        ("verification token", &secrets.verification_token),
        ("session ID", &secrets.session_id),
        ("MUID", &secrets.muid),
    ] {
        if value.is_empty() || value.contains(['\r', '\n']) {
            return Err(invalid(format!("The Forms {name} is missing or invalid")));
        }
    }
    let response = Client::builder()
        .redirect(Policy::none())
        .build()?
        .post(&profile.endpoint)
        .header("Accept", "application/json")
        .header("Accept-Language", &profile.language)
        .header("Cookie", &secrets.cookie)
        .header("Origin", &profile.origin)
        .header("Referer", &profile.referer)
        .header("__requestverificationtoken", &secrets.verification_token)
        .header("odata-version", "4.0")
        .header("odata-maxversion", "4.0")
        .header("x-correlationid", Uuid::new_v4().to_string())
        .header("x-ms-form-muid", &secrets.muid)
        .header("x-ms-form-request-ring", &profile.ring)
        .header("x-ms-form-request-source", "ms-formweb")
        .header("x-usersessionid", &secrets.session_id)
        .json(payload)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(invalid(format!(
            "Microsoft Forms returned HTTP {}; refresh the Forms connection before trying again",
            status.as_u16()
        )));
    }
    Ok(status)
}

pub fn preview_json(payload: &FormPayload) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&json!(payload))?)
}

fn table<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a Table> {
    value
        .and_then(Value::as_table)
        .ok_or_else(|| invalid(format!("{field} must be a TOML table")))
}

fn required(table: &Table, key: &str, section: &str) -> Result<String> {
    text(table.get(key), &format!("[{section}].{key}"))?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("Missing required [{section}].{key}")))
}

fn text(value: Option<&Value>, field: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("{field} must be text")))
        })
        .transpose()
}

fn forms_url(value: String, field: &str) -> Result<String> {
    let url = Url::parse(&value).map_err(|_| invalid(format!("{field} must be a Forms URL")))?;
    if url.scheme() != "https"
        || url.host_str() != Some("forms.cloud.microsoft")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(invalid(format!(
            "{field} must use https://forms.cloud.microsoft"
        )));
    }
    Ok(value)
}

fn env_name(table: &Table, key: &str, default: &str) -> Result<String> {
    let value =
        text(table.get(key), &format!("[auth].{key}"))?.unwrap_or_else(|| default.to_owned());
    if value.is_empty()
        || !value.chars().enumerate().all(|(index, character)| {
            character == '_'
                || character.is_ascii_uppercase()
                || index > 0 && character.is_ascii_digit()
        })
    {
        return Err(invalid(format!(
            "[auth].{key} must be an environment variable name"
        )));
    }
    Ok(value)
}

fn secret(name: &str) -> Result<String> {
    let value = env::var(name)
        .map_err(|_| invalid(format!("Missing required environment variable: {name}")))?;
    if value.contains(['\r', '\n']) {
        return Err(invalid(format!("{name} contains a newline")));
    }
    Ok(value)
}

fn configured_time(value: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|time| time.with_timezone(&Utc))
                .map_err(|_| invalid("Form timestamps must be ISO date-times with a time zone"))
        })
        .transpose()
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn expand(template: &str, context: &BTreeMap<String, String>) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail
            .find('}')
            .ok_or_else(|| invalid("Form profile has an unmatched opening brace"))?;
        let key = &tail[..end];
        result.push_str(
            context
                .get(key)
                .ok_or_else(|| invalid(format!("Unknown form-profile placeholder {{{key}}}")))?,
        );
        rest = &tail[end + 1..];
    }
    if rest.contains('}') {
        return Err(invalid("Form profile has an unmatched closing brace"));
    }
    result.push_str(rest);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn bundled_profile_builds_complete_payload() {
        let event = Event::load(Path::new("tests/movie-night.toml")).expect("sample event");
        let profile = FormProfile::bundled().expect("bundled profile");
        let now = DateTime::parse_from_rfc3339("2026-11-03T10:00:00Z")
            .expect("test timestamp")
            .with_timezone(&Utc);
        let payload = payload(&event, &profile, now).expect("form preview");
        let answers: Vec<Answer> = serde_json::from_str(&payload.answers).expect("answer JSON");
        assert!(answers.len() > 30);
        assert!(payload.answers.contains("German Club Movie Night"));
    }

    #[test]
    fn rejects_foreign_form_host() {
        let raw = PROFILE
            .replace(
                "https://forms.cloud.microsoft/formapi",
                "https://evil.test/formapi",
            )
            .parse::<Table>()
            .expect("modified profile");
        assert!(FormProfile::from_table(&raw).is_err());
    }
}
