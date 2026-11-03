use std::{fs, future::Future, io::Cursor, path::Path, sync::OnceLock};

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use zip::ZipArchive;

use crate::{
    cache::ArtifactCache,
    db::Store,
    error::{IoContext, Result, SocError, invalid},
    graph::{CLUBS_SENDER, Graph, GraphError, GraphResult, Message, RECIPIENTS, REQUEST_SENDER},
    model::{Event, RiskLayout},
    risk,
};

#[derive(Clone, Debug)]
struct SavedEvent {
    owner: String,
    event_id: String,
    title: String,
    filename: String,
    document: Vec<u8>,
    since: String,
    state: String,
    request_id: Option<String>,
    draft_id: Option<String>,
    send_started: Option<String>,
    sent_at: Option<String>,
    update_thread: Option<String>,
    update_time: Option<String>,
    reply_id: Option<String>,
    error: String,
    recipient: String,
    subject: String,
}

impl SavedEvent {
    fn row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            owner: row.get("owner")?,
            event_id: row.get("event_id")?,
            title: row.get("title")?,
            filename: row.get("filename")?,
            document: row.get("document")?,
            since: row.get("since")?,
            state: row.get("state")?,
            request_id: row.get("request_id")?,
            draft_id: row.get("draft_id")?,
            send_started: row.get("send_started")?,
            sent_at: row.get("sent_at")?,
            update_thread: row.get("update_thread")?,
            update_time: row.get("update_time")?,
            reply_id: row.get("reply_id")?,
            error: row.get("error")?,
            recipient: row.get("recipient")?,
            subject: row.get("subject")?,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EventStatus {
    pub event_id: String,
    pub title: String,
    pub state: String,
    pub outcome: String,
    pub details: String,
    pub link: String,
    pub error: String,
    pub updated_at: String,
}

pub struct Watch<'a> {
    pub source: &'a Path,
    pub event_id: &'a str,
    pub attachment: Option<&'a Path>,
    pub recipient: &'a str,
    pub since: Option<&'a str>,
}

pub trait Mail {
    fn messages(&self, since: &str) -> impl Future<Output = GraphResult<Vec<Message>>> + Send;
    fn message(&self, id: &str) -> impl Future<Output = GraphResult<Message>> + Send;
    fn create_reply(&self, id: &str) -> impl Future<Output = GraphResult<String>> + Send;
    fn prepare(
        &self,
        draft: &str,
        subject: &str,
        recipient: &str,
        filename: &str,
        document: &[u8],
    ) -> impl Future<Output = GraphResult<()>> + Send;
    fn send(&self, draft: &str) -> impl Future<Output = GraphResult<()>> + Send;
}

impl Mail for Graph {
    async fn messages(&self, since: &str) -> GraphResult<Vec<Message>> {
        self.messages(since).await
    }

    async fn message(&self, id: &str) -> GraphResult<Message> {
        self.message(id).await
    }

    async fn create_reply(&self, id: &str) -> GraphResult<String> {
        self.create_reply(id).await
    }

    async fn prepare(
        &self,
        draft: &str,
        subject: &str,
        recipient: &str,
        filename: &str,
        document: &[u8],
    ) -> GraphResult<()> {
        self.prepare(draft, subject, recipient, filename, document)
            .await
    }

    async fn send(&self, draft: &str) -> GraphResult<()> {
        self.send(draft).await
    }
}

pub fn watch(
    store: &Store,
    cache: &ArtifactCache,
    owner: &str,
    options: &Watch<'_>,
) -> Result<EventStatus> {
    if !event_id().is_match(options.event_id) {
        return Err(invalid(
            "Event ID must be 1–80 letters, digits, underscores or hyphens",
        ));
    }
    if !RECIPIENTS.contains(&options.recipient) {
        return Err(invalid(
            "Choose pfeventa@uq.edu.au or pfassist@pf.uq.edu.au for the risk return",
        ));
    }
    let event = Event::load(options.source)?;
    let cutoff = match options.since {
        Some(value) => {
            let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| invalid("--since must be YYYY-MM-DD"))?;
            date.and_hms_opt(0, 0, 0)
                .ok_or_else(|| invalid("--since must be YYYY-MM-DD"))?
                .and_utc()
        }
        None => Utc::now() - Duration::days(30),
    };
    if cutoff > Utc::now() {
        return Err(invalid("--since cannot be in the future"));
    }
    let (filename, document) = match options.attachment {
        Some(path) => (
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid("The risk assessment needs a plain UTF-8 filename"))?
                .to_owned(),
            fs::read(path).at_path("Cannot read", path)?,
        ),
        None => {
            let key = cache.key("risk-docx", &(&event, RiskLayout::Form))?;
            (
                format!("{}.docx", event.output_name),
                cache.get_or_build(&key, "docx", || risk::document(&event, RiskLayout::Form))?,
            )
        }
    };
    validate_document(&filename, &document)?;
    let now = Utc::now().to_rfc3339();
    let subject = format!("UQU Risk Assessment Return Ref ID:{}", options.event_id);
    store.with(|connection| {
        connection.execute(
            "INSERT INTO events (owner,event_id,title,filename,document,since,state,recipient,subject,updated_at) VALUES (?1,?2,?3,?4,?5,?6,'waiting_request',?7,?8,?9)",
            params![
                owner,
                options.event_id,
                event.title,
                filename,
                document,
                cutoff.to_rfc3339(),
                options.recipient,
                subject,
                now,
            ],
        )
        .map_err(|error| {
            if matches!(error, rusqlite::Error::SqliteFailure(ref detail, _) if detail.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY) {
                invalid(format!("Event {} is already being followed", options.event_id))
            } else {
                SocError::from(error)
            }
        })?;
        Ok(())
    })?;
    status_one(store, owner, options.event_id)
}

pub fn status(store: &Store, owner: &str) -> Result<Vec<EventStatus>> {
    store.with(|connection| {
        let mut statement = connection.prepare(
            "SELECT event_id,title,state,coalesce(outcome,''),details,link,error,updated_at FROM events WHERE owner = ?1 ORDER BY updated_at DESC,event_id",
        )?;
        statement
            .query_map([owner], |row| {
                Ok(EventStatus {
                    event_id: row.get(0)?,
                    title: row.get(1)?,
                    state: row.get(2)?,
                    outcome: row.get(3)?,
                    details: row.get(4)?,
                    link: row.get(5)?,
                    error: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SocError::from)
    })
}

pub fn stop(store: &Store, owner: &str, id: &str) -> Result<()> {
    changed(
        store,
        owner,
        id,
        "UPDATE events SET state='stopped',error='',updated_at=?3 WHERE owner=?1 AND event_id=?2",
    )
}

pub fn retry_send(store: &Store, owner: &str, id: &str, confirmed_not_sent: bool) -> Result<()> {
    if !confirmed_not_sent {
        return Err(invalid(
            "Check Outlook Sent Items and pass --confirm-not-sent before allowing a retry",
        ));
    }
    let event = load(store, owner, id)?;
    if event.state != "sending" || !event.error.contains("uncertain") {
        return Err(invalid(
            "Only an uncertain send can be retried after checking Sent Items",
        ));
    }
    changed(
        store,
        owner,
        id,
        "UPDATE events SET state='draft',send_started=NULL,error='',updated_at=?3 WHERE owner=?1 AND event_id=?2",
    )
}

pub async fn poll(store: &Store, owner: &str) -> Result<()> {
    let pending = pending(store, owner)?;
    if pending.is_empty() {
        return Ok(());
    }
    let graph = crate::graph::connect(store, owner).await?;
    poll_with(store, owner, &graph, pending).await
}

pub async fn poll_all(store: &Store) -> Result<()> {
    let owners = store.with(|connection| {
        let mut statement = connection.prepare(
            "SELECT DISTINCT owner FROM events WHERE state NOT IN ('stopped','complete') ORDER BY owner",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SocError::from)
    })?;
    for owner in owners {
        if let Err(error) = poll(store, &owner).await {
            set_owner_error(store, &owner, &error.to_string())?;
        }
    }
    Ok(())
}

async fn poll_with<M: Mail>(
    store: &Store,
    owner: &str,
    mail: &M,
    events: Vec<SavedEvent>,
) -> Result<()> {
    let since = events
        .iter()
        .map(|event| event.since.as_str())
        .min()
        .ok_or_else(|| invalid("No pending event cutoff"))?;
    let messages = mail
        .messages(since)
        .await
        .map_err(|error| invalid(error.to_string()))?;
    for event in events {
        if let Err(error) = advance(store, mail, event, &messages).await {
            set_error(store, owner, &error.event_id, &error.message)?;
            if error.rate_limited {
                break;
            }
        }
    }
    Ok(())
}

struct AdvanceError {
    event_id: String,
    message: String,
    rate_limited: bool,
}

impl AdvanceError {
    fn graph(event: &SavedEvent, error: GraphError) -> Self {
        Self {
            event_id: event.event_id.clone(),
            message: error.to_string(),
            rate_limited: error.status == Some(reqwest::StatusCode::TOO_MANY_REQUESTS),
        }
    }
}

async fn advance<M: Mail>(
    store: &Store,
    mail: &M,
    mut event: SavedEvent,
    messages: &[Message],
) -> std::result::Result<(), AdvanceError> {
    let result = async {
        if event.state == "waiting_request" {
            let request = messages
                .iter()
                .filter(|message| matches(message, &event, true))
                .filter(|message| later(&message.received_date_time, &event.since).unwrap_or(false))
                .min_by_key(|message| &message.received_date_time);
            let Some(request) = request else {
                return Ok(());
            };
            store.with(|connection| {
                connection.execute(
                    "UPDATE events SET state='draft',request_id=?3,request_time=?4,error='',updated_at=?5 WHERE owner=?1 AND event_id=?2",
                    params![event.owner, event.event_id, request.id, request.received_date_time, Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })?;
            event = load(store, &event.owner, &event.event_id)?;
        }
        if event.state == "draft" {
            let request_id = event
                .request_id
                .as_deref()
                .ok_or_else(|| invalid("The saved booking request is missing"))?;
            let draft_id = match &event.draft_id {
                Some(value) => value.clone(),
                None => {
                    let value = mail
                        .create_reply(request_id)
                        .await
                        .map_err(|error| AdvanceError::graph(&event, error))?;
                    store.with(|connection| {
                        connection.execute(
                            "UPDATE events SET draft_id=?3,updated_at=?4 WHERE owner=?1 AND event_id=?2",
                            params![event.owner, event.event_id, value, Utc::now().to_rfc3339()],
                        )?;
                        Ok(())
                    })?;
                    value
                }
            };
            let draft = mail
                .message(&draft_id)
                .await
                .map_err(|error| AdvanceError::graph(&event, error))?;
            if draft.is_draft == Some(false) && !draft.sent_date_time.is_empty() {
                record_sent(store, &event, &draft)?;
            } else if draft.is_draft == Some(true) {
                mail.prepare(
                    &draft_id,
                    &event.subject,
                    &event.recipient,
                    &event.filename,
                    &event.document,
                )
                .await
                .map_err(|error| AdvanceError::graph(&event, error))?;
                store.with(|connection| {
                    connection.execute(
                        "UPDATE events SET state='sending',send_started=?3,error='',updated_at=?3 WHERE owner=?1 AND event_id=?2",
                        params![event.owner, event.event_id, Utc::now().to_rfc3339()],
                    )?;
                    Ok(())
                })?;
                if let Err(error) = mail.send(&draft_id).await {
                    if error.status.is_some_and(|status| {
                        [400, 401, 403, 413, 429].contains(&status.as_u16())
                    }) {
                        changed(store, &event.owner, &event.event_id, "UPDATE events SET state='draft',updated_at=?3 WHERE owner=?1 AND event_id=?2")?;
                    }
                    return Err(AdvanceError::graph(&event, error));
                }
            } else {
                return Err(invalid("Microsoft returned no draft or send status").into());
            }
            event = load(store, &event.owner, &event.event_id)?;
        }
        if event.state == "sending" {
            let draft_id = event
                .draft_id
                .as_deref()
                .ok_or_else(|| invalid("The saved draft identity is missing"))?;
            match mail.message(draft_id).await {
                Ok(sent) if sent.is_draft == Some(false) && !sent.sent_date_time.is_empty() => {
                    record_sent(store, &event, &sent)?;
                }
                Ok(_) => {
                    uncertain_if_expired(store, &event)?;
                    return Ok(());
                }
                Err(error) if error.status == Some(reqwest::StatusCode::NOT_FOUND) => {
                    uncertain_if_expired(store, &event)?;
                    return Ok(());
                }
                Err(error) => return Err(AdvanceError::graph(&event, error)),
            }
            event = load(store, &event.owner, &event.event_id)?;
        }
        if ["waiting_update", "waiting_reply"].contains(&event.state.as_str()) {
            let updates = messages
                .iter()
                .filter(|message| matches(message, &event, false))
                .filter(|message| {
                    event
                        .sent_at
                        .as_deref()
                        .is_some_and(|sent| later(&message.received_date_time, sent).unwrap_or(false))
                })
                .collect::<Vec<_>>();
            if let Some(update) = updates
                .into_iter()
                .max_by_key(|message| &message.received_date_time)
            {
                if update.conversation_id.is_empty() {
                    return Err(invalid("The UQ update has no conversation ID").into());
                }
                let newer = event
                    .update_time
                    .as_deref()
                    .is_none_or(|time| later(&update.received_date_time, time).unwrap_or(false));
                if newer {
                    store.with(|connection| {
                        connection.execute(
                            "UPDATE events SET state='waiting_reply',update_id=?3,update_thread=?4,update_time=?5,reply_id=NULL,outcome=NULL,details='',link='',error='',updated_at=?6 WHERE owner=?1 AND event_id=?2",
                            params![event.owner, event.event_id, update.id, update.conversation_id, update.received_date_time, Utc::now().to_rfc3339()],
                        )?;
                        Ok(())
                    })?;
                    event = load(store, &event.owner, &event.event_id)?;
                }
            }
        }
        if event.state == "waiting_reply" {
            let Some(thread) = event.update_thread.as_deref() else {
                return Ok(());
            };
            let Some(update_time) = event.update_time.as_deref() else {
                return Ok(());
            };
            let reply = messages
                .iter()
                .filter(|message| sender(message) == CLUBS_SENDER)
                .filter(|message| message.conversation_id == thread)
                .filter(|message| later(&message.received_date_time, update_time).unwrap_or(false))
                .filter(|message| {
                    parse_subject(&message.subject)
                        .is_none_or(|(_, id, _)| id == event.event_id)
                })
                .max_by_key(|message| &message.received_date_time);
            let Some(reply) = reply else {
                return Ok(());
            };
            if event.reply_id.as_deref() != Some(&reply.id) {
                let detail = mail
                    .message(&reply.id)
                    .await
                    .map_err(|error| AdvanceError::graph(&event, error))?;
                let content = detail.unique_body.content.trim();
                let outcome = if detail.unique_body.content_type.eq_ignore_ascii_case("text") {
                    booking_outcome(content)
                } else {
                    "review"
                };
                let state = if outcome == "review" {
                    "waiting_reply"
                } else {
                    "complete"
                };
                store.with(|connection| {
                    connection.execute(
                        "UPDATE events SET reply_id=?3,outcome=?4,details=?5,link=?6,error='',state=?7,updated_at=?8 WHERE owner=?1 AND event_id=?2",
                        params![event.owner, event.event_id, reply.id, outcome, content.chars().take(20_000).collect::<String>(), detail.web_link, state, Utc::now().to_rfc3339()],
                    )?;
                    Ok(())
                })?;
            }
        }
        Ok(())
    }
    .await;
    result.map_err(|mut error| {
        if error.event_id.is_empty() {
            error.event_id.clone_from(&event.event_id);
        }
        error
    })
}

impl From<SocError> for AdvanceError {
    fn from(error: SocError) -> Self {
        Self {
            event_id: String::new(),
            message: error.to_string(),
            rate_limited: false,
        }
    }
}

fn pending(store: &Store, owner: &str) -> Result<Vec<SavedEvent>> {
    store.with(|connection| {
        let mut statement = connection.prepare(
            "SELECT * FROM events WHERE owner=?1 AND state NOT IN ('stopped','complete') ORDER BY since,event_id",
        )?;
        statement
            .query_map([owner], SavedEvent::row)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SocError::from)
    })
}

fn load(store: &Store, owner: &str, id: &str) -> Result<SavedEvent> {
    store.with(|connection| {
        connection
            .query_row(
                "SELECT * FROM events WHERE owner=?1 AND event_id=?2",
                params![owner, id],
                SavedEvent::row,
            )
            .optional()?
            .ok_or_else(|| invalid(format!("Unknown event ID: {id}")))
    })
}

fn status_one(store: &Store, owner: &str, id: &str) -> Result<EventStatus> {
    status(store, owner)?
        .into_iter()
        .find(|event| event.event_id == id)
        .ok_or_else(|| invalid(format!("Unknown event ID: {id}")))
}

fn changed(store: &Store, owner: &str, id: &str, query: &str) -> Result<()> {
    store.with(|connection| {
        let changed = connection.execute(query, params![owner, id, Utc::now().to_rfc3339()])?;
        if changed == 0 {
            return Err(invalid(format!("Unknown event ID: {id}")));
        }
        Ok(())
    })
}

fn set_error(store: &Store, owner: &str, id: &str, message: &str) -> Result<()> {
    store.with(|connection| {
        connection.execute(
            "UPDATE events SET error=?3,updated_at=?4 WHERE owner=?1 AND event_id=?2",
            params![owner, id, message, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })
}

fn set_owner_error(store: &Store, owner: &str, message: &str) -> Result<()> {
    store.with(|connection| {
        connection.execute(
            "UPDATE events SET error=?2,updated_at=?3 WHERE owner=?1 AND state NOT IN ('stopped','complete')",
            params![owner, message, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })
}

fn record_sent(store: &Store, event: &SavedEvent, sent: &Message) -> Result<()> {
    let recipients = sent
        .to_recipients
        .iter()
        .map(|recipient| recipient.email_address.address.to_lowercase())
        .collect::<Vec<_>>();
    if sent.subject != event.subject
        || recipients != [event.recipient.to_lowercase()]
        || !sent.cc_recipients.is_empty()
        || !sent.bcc_recipients.is_empty()
    {
        return Err(invalid(
            "The sent draft's subject or recipients changed; inspect it in Outlook",
        ));
    }
    time(&sent.sent_date_time)?;
    store.with(|connection| {
        connection.execute(
            "UPDATE events SET state='waiting_update',sent_at=?3,error='',updated_at=?4 WHERE owner=?1 AND event_id=?2",
            params![event.owner, event.event_id, sent.sent_date_time, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })
}

fn uncertain_if_expired(store: &Store, event: &SavedEvent) -> Result<()> {
    let Some(started) = event.send_started.as_deref() else {
        return Err(invalid("The saved send time is missing"));
    };
    if Utc::now() - time(started)? > Duration::minutes(10) {
        set_error(
            store,
            &event.owner,
            &event.event_id,
            "Send status is uncertain. Check Outlook Sent Items before using agent retry-send",
        )?;
    }
    Ok(())
}

fn matches(message: &Message, event: &SavedEvent, request: bool) -> bool {
    let Some((is_request, id, title)) = parse_subject(&message.subject) else {
        return false;
    };
    sender(message) == REQUEST_SENDER
        && is_request == request
        && id == event.event_id
        && normalise(&title) == normalise(&event.title)
}

fn sender(message: &Message) -> String {
    message
        .from
        .as_ref()
        .map(|sender| sender.email_address.address.to_lowercase())
        .unwrap_or_default()
}

fn parse_subject(value: &str) -> Option<(bool, String, String)> {
    subject().captures(value).and_then(|captures| {
        Some((
            captures.name("request").is_some(),
            captures.name("id")?.as_str().to_owned(),
            captures.name("title")?.as_str().trim().to_owned(),
        ))
    })
}

pub fn booking_outcome(content: &str) -> &'static str {
    let mut outcomes = std::collections::BTreeSet::new();
    for sentence in content.split(['.', '!', '?', '\n']).map(str::to_lowercase) {
        if sentence.contains("not booked")
            || sentence.contains("booking unsuccessful")
            || sentence.contains("unable to book")
        {
            outcomes.insert("not_booked");
            continue;
        }
        if [" not ", " no ", "can't", "cannot", "requested"]
            .iter()
            .any(|word| sentence.contains(word))
        {
            continue;
        }
        if [
            "unavailable",
            "declined",
            "rejected",
            "cancelled",
            "canceled",
        ]
        .iter()
        .any(|word| sentence.contains(word))
        {
            outcomes.insert("not_booked");
        }
        if ["changed", "moved", "relocated", "rescheduled"]
            .iter()
            .any(|word| sentence.contains(word))
        {
            outcomes.insert("changed");
        }
        if ["booked", "confirmed", "secured", "approved"]
            .iter()
            .any(|word| sentence.contains(word))
        {
            outcomes.insert("booked");
        }
    }
    if outcomes.len() == 1 {
        outcomes.first().copied().unwrap_or("review")
    } else {
        "review"
    }
}

fn validate_document(filename: &str, document: &[u8]) -> Result<()> {
    if document.is_empty() || document.len() >= 3_000_000 {
        return Err(invalid(
            "Risk assessment must be non-empty and smaller than 3 MB",
        ));
    }
    let valid = if filename.to_lowercase().ends_with(".pdf") {
        document.starts_with(b"%PDF-")
    } else if filename.to_lowercase().ends_with(".docx") {
        ZipArchive::new(Cursor::new(document))
            .ok()
            .is_some_and(|mut archive| {
                let mut content_types = false;
                let mut document_xml = false;
                for index in 0..archive.len() {
                    if let Ok(file) = archive.by_index(index) {
                        content_types |= file.name() == "[Content_Types].xml";
                        document_xml |= file.name() == "word/document.xml";
                    }
                }
                content_types && document_xml
            })
    } else {
        false
    };
    if !valid {
        return Err(invalid("Attach a valid DOCX or PDF risk assessment"));
    }
    Ok(())
}

fn later(candidate: &str, reference: &str) -> Result<bool> {
    Ok(time(candidate)? >= time(reference)?)
}

fn time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| invalid("Microsoft returned an invalid message timestamp"))
}

fn normalise(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn event_id() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$").expect("event ID regex"))
}

fn subject() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?i)^Event Application Update(?P<request> Risk Assessment Request)?\s+\[Event ID:\s*(?P<id>[A-Za-z0-9][A-Za-z0-9_-]{0,79})\]\s*-\s*(?P<title>.+)$")
            .expect("booking subject regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_booking_language_without_guessing_conflicts() {
        assert_eq!(booking_outcome("Your room is booked"), "booked");
        assert_eq!(booking_outcome("The room is unavailable"), "not_booked");
        assert_eq!(
            booking_outcome("Your booking moved to another room"),
            "changed"
        );
        assert_eq!(booking_outcome("We requested confirmation"), "review");
        assert_eq!(booking_outcome("Booked, then cancelled"), "review");
    }

    #[test]
    fn parses_only_exact_booking_subjects() {
        let parsed = parse_subject(
            "Event Application Update Risk Assessment Request [Event ID: 42] - Movie Night",
        )
        .expect("subject");
        assert!(parsed.0);
        assert_eq!(parsed.1, "42");
        assert_eq!(parsed.2, "Movie Night");
        assert!(
            parse_subject("Re: Event Application Update [Event ID: 42] - Movie Night").is_none()
        );
    }
}
