use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, TimeDelta, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Client, Method, StatusCode, Url, redirect::Policy};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    db::Store,
    error::{Result, SocError, invalid},
};

pub const REQUEST_SENDER: &str = "pfeventapplication@uq.edu.au";
pub const CLUBS_SENDER: &str = "clubs@uqu.com.au";
pub const RETURN_TO: &str = "pfeventa@uq.edu.au";
pub const RECIPIENTS: [&str; 2] = [RETURN_TO, "pfassist@pf.uq.edu.au"];
pub const BODY: &str = "Please find the approved risk assessment attached.\n\nDo not change the subject line because its reference ID is linked to the event application.\n\nThank you";
const SCOPES: &str = "offline_access Mail.ReadWrite Mail.Send User.Read";
const GRAPH: &str = "https://graph.microsoft.com/v1.0";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    pub address: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipient {
    pub email_address: EmailAddress,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub from: Option<Recipient>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub conversation_id: String,
    #[serde(default)]
    pub received_date_time: String,
    #[serde(default)]
    pub is_draft: Option<bool>,
    #[serde(default)]
    pub sent_date_time: String,
    #[serde(default)]
    pub to_recipients: Vec<Recipient>,
    #[serde(default)]
    pub cc_recipients: Vec<Recipient>,
    #[serde(default)]
    pub bcc_recipients: Vec<Recipient>,
    #[serde(default)]
    pub unique_body: Body,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub web_link: String,
}

#[derive(Debug)]
pub struct GraphError {
    pub status: Option<StatusCode>,
    pub retry_after: u64,
    pub message: String,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GraphError {}

impl From<reqwest::Error> for GraphError {
    fn from(error: reqwest::Error) -> Self {
        Self {
            status: error.status(),
            retry_after: 60,
            message: "Microsoft Graph is unavailable; the watcher will try again".into(),
        }
    }
}

pub type GraphResult<T> = std::result::Result<T, GraphError>;

#[derive(Clone)]
pub struct Graph {
    client: Client,
    token: String,
}

impl Graph {
    pub fn new(token: String) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(30))
                .build()?,
            token,
        })
    }

    pub async fn me(&self) -> GraphResult<String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Me {
            #[serde(default)]
            mail: String,
            #[serde(default)]
            user_principal_name: String,
        }
        let me: Me = self
            .request(Method::GET, "/me?$select=mail,userPrincipalName", None)
            .await?;
        Ok(if me.mail.is_empty() {
            me.user_principal_name
        } else {
            me.mail
        }
        .to_lowercase())
    }

    pub async fn messages(&self, since: &str) -> GraphResult<Vec<Message>> {
        DateTime::parse_from_rfc3339(since).map_err(|_| GraphError {
            status: None,
            retry_after: 60,
            message: "The saved mailbox cutoff is invalid".into(),
        })?;
        let filter = format!(
            "receivedDateTime ge {since} and isDraft eq false and (from/emailAddress/address eq '{REQUEST_SENDER}' or from/emailAddress/address eq '{CLUBS_SENDER}')"
        );
        let mut url = Url::parse(&format!("{GRAPH}/me/messages")).map_err(|error| GraphError {
            status: None,
            retry_after: 60,
            message: error.to_string(),
        })?;
        url.query_pairs_mut()
            .append_pair("$filter", &filter)
            .append_pair("$select", "id,from,subject,conversationId,receivedDateTime")
            .append_pair("$top", "100");
        self.pages(url).await
    }

    pub async fn message(&self, id: &str) -> GraphResult<Message> {
        self.request(
            Method::GET,
            &format!(
                "/me/messages/{}?$select=id,isDraft,sentDateTime,subject,toRecipients,ccRecipients,bccRecipients,uniqueBody,body,webLink,conversationId,from,receivedDateTime",
                path_id(id)
            ),
            None,
        )
        .await
    }

    pub async fn create_reply(&self, id: &str) -> GraphResult<String> {
        let message: Message = self
            .request(
                Method::POST,
                &format!("/me/messages/{}/createReply", path_id(id)),
                Some(json!({})),
            )
            .await?;
        Ok(message.id)
    }

    pub async fn prepare(
        &self,
        draft: &str,
        subject: &str,
        recipient: &str,
        filename: &str,
        document: &[u8],
    ) -> GraphResult<()> {
        let path = format!("/me/messages/{}", path_id(draft));
        let _: Value = self
            .request(
                Method::PATCH,
                &path,
                Some(json!({
                    "subject": subject,
                    "body": {"contentType": "Text", "content": BODY},
                    "toRecipients": [{"emailAddress": {"address": recipient}}],
                    "ccRecipients": [],
                    "bccRecipients": []
                })),
            )
            .await?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Attachment {
            #[serde(default)]
            name: String,
            #[serde(default)]
            content_bytes: String,
        }
        let attachments: Vec<Attachment> = self
            .pages(
                Url::parse(&format!("{GRAPH}{path}/attachments")).map_err(|error| GraphError {
                    status: None,
                    retry_after: 60,
                    message: error.to_string(),
                })?,
            )
            .await?;
        let encoded = STANDARD.encode(document);
        if let Some(existing) = attachments.first() {
            if attachments.len() != 1
                || existing.name != filename
                || existing.content_bytes != encoded
            {
                return Err(GraphError {
                    status: None,
                    retry_after: 60,
                    message: "The saved draft has unexpected attachments; inspect it in Outlook"
                        .into(),
                });
            }
            return Ok(());
        }
        let _: Value = self
            .request(
                Method::POST,
                &format!("{path}/attachments"),
                Some(json!({
                    "@odata.type": "#microsoft.graph.fileAttachment",
                    "name": filename,
                    "contentType": if filename.to_lowercase().ends_with(".pdf") {
                        "application/pdf"
                    } else {
                        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    },
                    "contentBytes": encoded
                })),
            )
            .await?;
        Ok(())
    }

    pub async fn send(&self, draft: &str) -> GraphResult<()> {
        let _: Value = self
            .request(
                Method::POST,
                &format!("/me/messages/{}/send", path_id(draft)),
                Some(json!({})),
            )
            .await?;
        Ok(())
    }

    async fn pages<T: DeserializeOwned>(&self, mut url: Url) -> GraphResult<Vec<T>> {
        #[derive(Deserialize)]
        struct Page<T> {
            value: Vec<T>,
            #[serde(rename = "@odata.nextLink", default)]
            next: String,
        }
        let mut values = Vec::new();
        loop {
            validate_graph_url(&url)?;
            let page: Page<T> = self.request_url(Method::GET, url, None).await?;
            values.extend(page.value);
            if page.next.is_empty() {
                break;
            }
            url = Url::parse(&page.next).map_err(|_| GraphError {
                status: None,
                retry_after: 60,
                message: "Microsoft returned an invalid pagination link".into(),
            })?;
        }
        Ok(values)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> GraphResult<T> {
        let url = Url::parse(&format!("{GRAPH}{path}")).map_err(|_| GraphError {
            status: None,
            retry_after: 60,
            message: "Refusing an invalid Graph path".into(),
        })?;
        self.request_url(method, url, body).await
    }

    async fn request_url<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> GraphResult<T> {
        validate_graph_url(&url)?;
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(&self.token)
            .header(
                "Prefer",
                "IdType=\"ImmutableId\", outlook.body-content-type=\"text\"",
            );
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(60)
                .max(30);
            return Err(GraphError {
                status: Some(response.status()),
                retry_after,
                message: format!(
                    "Microsoft Graph returned HTTP {}",
                    response.status().as_u16()
                ),
            });
        }
        if response.status() == StatusCode::NO_CONTENT {
            serde_json::from_value(Value::Null).map_err(|error| GraphError {
                status: None,
                retry_after: 60,
                message: error.to_string(),
            })
        } else {
            response.json().await.map_err(GraphError::from)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceFlow {
    pub id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at: String,
    pub interval_seconds: u64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeviceStatus {
    Pending { retry_after: u64 },
    Connected { mailbox: String },
}

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i64,
    interval: u64,
    message: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct TokenFailure {
    error: String,
    #[serde(default)]
    error_description: String,
}

pub async fn start_device_login(
    store: &Store,
    owner: &str,
    mailbox: &str,
    client_id: &str,
    tenant_id: &str,
) -> Result<DeviceFlow> {
    let (mailbox, client_id, tenant_id) = validate_account(mailbox, client_id, tenant_id)?;
    let endpoint = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/devicecode");
    let response = auth_client()?
        .post(endpoint)
        .form(&[("client_id", client_id.as_str()), ("scope", SCOPES)])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(invalid(format!(
            "Microsoft could not start sign-in (HTTP {})",
            response.status().as_u16()
        )));
    }
    let response: DeviceResponse = response.json().await?;
    let now = Utc::now();
    let expires_at = now
        .checked_add_signed(TimeDelta::seconds(response.expires_in))
        .ok_or_else(|| invalid("Microsoft returned an invalid device-code lifetime"))?;
    let id = Uuid::new_v4().to_string();
    let encrypted = store.seal(&response.device_code)?;
    store.with(|connection| {
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM device_flows WHERE owner = ?1", [owner])?;
        transaction.execute(
            "INSERT INTO device_flows (id,owner,mailbox,client_id,tenant_id,device_code,user_code,verification_uri,expires_at,interval_seconds,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id,
                owner,
                mailbox,
                client_id,
                tenant_id,
                encrypted,
                response.user_code,
                response.verification_uri,
                expires_at.to_rfc3339(),
                i64::try_from(response.interval)
                    .map_err(|_| invalid("Microsoft returned an invalid polling interval"))?,
                now.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    })?;
    Ok(DeviceFlow {
        id,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_at: expires_at.to_rfc3339(),
        interval_seconds: response.interval,
        message: response.message,
    })
}

pub async fn poll_device_login(store: &Store, owner: &str, flow_id: &str) -> Result<DeviceStatus> {
    #[allow(clippy::type_complexity)]
    let flow: Option<(String, String, String, Vec<u8>, String, i64)> = store.with(|connection| {
        connection
            .query_row(
                "SELECT mailbox,client_id,tenant_id,device_code,expires_at,interval_seconds FROM device_flows WHERE id = ?1 AND owner = ?2",
                params![flow_id, owner],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .optional()
            .map_err(SocError::from)
    })?;
    let Some((mailbox, client_id, tenant_id, encrypted, expires_at, interval)) = flow else {
        return Err(invalid("That Microsoft sign-in is no longer active"));
    };
    let expiry = DateTime::parse_from_rfc3339(&expires_at)
        .map_err(|_| invalid("The saved Microsoft sign-in expiry is invalid"))?;
    if expiry < Utc::now() {
        store.with(|connection| {
            connection.execute(
                "DELETE FROM device_flows WHERE id = ?1 AND owner = ?2",
                params![flow_id, owner],
            )?;
            Ok(())
        })?;
        return Err(invalid("The Microsoft sign-in code expired; start again"));
    }
    let device_code = store.open_secret(&encrypted)?;
    let endpoint = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token");
    let response = auth_client()?
        .post(endpoint)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.as_str()),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await?;
    if response.status().is_success() {
        let tokens: TokenResponse = response.json().await?;
        let graph = Graph::new(tokens.access_token)?;
        let signed_in = graph
            .me()
            .await
            .map_err(|error| invalid(error.to_string()))?;
        if signed_in != mailbox {
            return Err(invalid(format!(
                "Signed into {signed_in}; use the registered mailbox {mailbox}"
            )));
        }
        save_account(
            store,
            owner,
            &mailbox,
            &client_id,
            &tenant_id,
            &tokens.refresh_token,
        )?;
        store.with(|connection| {
            connection.execute(
                "DELETE FROM device_flows WHERE id = ?1 AND owner = ?2",
                params![flow_id, owner],
            )?;
            Ok(())
        })?;
        return Ok(DeviceStatus::Connected { mailbox });
    }
    let failure: TokenFailure = response.json().await?;
    match failure.error.as_str() {
        "authorization_pending" => Ok(DeviceStatus::Pending {
            retry_after: u64::try_from(interval).unwrap_or(5),
        }),
        "slow_down" => Ok(DeviceStatus::Pending {
            retry_after: u64::try_from(interval).unwrap_or(5) + 5,
        }),
        "authorization_declined" => Err(invalid("Microsoft sign-in was cancelled")),
        "expired_token" | "bad_verification_code" => {
            Err(invalid("The Microsoft sign-in code expired; start again"))
        }
        _ => Err(invalid(if failure.error_description.is_empty() {
            "Microsoft sign-in failed".into()
        } else {
            failure.error_description
        })),
    }
}

pub async fn connect(store: &Store, owner: &str) -> Result<Graph> {
    let account: Option<(String, String, String, Vec<u8>)> = store.with(|connection| {
        connection
            .query_row(
                "SELECT mailbox,client_id,tenant_id,refresh_token FROM accounts WHERE owner = ?1",
                [owner],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(SocError::from)
    })?;
    let Some((mailbox, client_id, tenant_id, encrypted)) = account else {
        return Err(invalid(
            "Connect Microsoft before following booking replies",
        ));
    };
    let refresh_token = store.open_secret(&encrypted)?;
    let endpoint = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token");
    let response = auth_client()?
        .post(endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("scope", SCOPES),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(invalid(
            "Microsoft sign-in needs attention; connect the mailbox again",
        ));
    }
    let tokens: TokenResponse = response.json().await?;
    let _ = tokens.expires_in;
    if !tokens.refresh_token.is_empty() {
        save_account(
            store,
            owner,
            &mailbox,
            &client_id,
            &tenant_id,
            &tokens.refresh_token,
        )?;
    }
    Graph::new(tokens.access_token)
}

pub fn connected_mailbox(store: &Store, owner: &str) -> Result<Option<String>> {
    store.with(|connection| {
        connection
            .query_row(
                "SELECT mailbox FROM accounts WHERE owner = ?1",
                [owner],
                |row| row.get(0),
            )
            .optional()
            .map_err(SocError::from)
    })
}

fn save_account(
    store: &Store,
    owner: &str,
    mailbox: &str,
    client_id: &str,
    tenant_id: &str,
    refresh_token: &str,
) -> Result<()> {
    let encrypted = store.seal(refresh_token)?;
    store.with(|connection| {
        connection.execute(
            "INSERT INTO accounts (owner,mailbox,client_id,tenant_id,refresh_token,updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(owner) DO UPDATE SET mailbox=excluded.mailbox,client_id=excluded.client_id,tenant_id=excluded.tenant_id,refresh_token=excluded.refresh_token,updated_at=excluded.updated_at",
            params![owner, mailbox, client_id, tenant_id, encrypted, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })
}

fn validate_account(
    mailbox: &str,
    client_id: &str,
    tenant_id: &str,
) -> Result<(String, String, String)> {
    let client_id = Uuid::parse_str(client_id)
        .map_err(|_| invalid("Microsoft client ID must be a UUID"))?
        .to_string();
    let tenant_id = Uuid::parse_str(tenant_id)
        .map_err(|_| invalid("Microsoft tenant ID must be a UUID"))?
        .to_string();
    let mailbox = mailbox.trim().to_lowercase();
    let parts = mailbox.split('@').collect::<Vec<_>>();
    if parts.len() != 2
        || parts
            .iter()
            .any(|part| part.is_empty() || part.chars().any(char::is_whitespace))
        || !parts[1].contains('.')
    {
        return Err(invalid("Use the Microsoft 365 mailbox email address"));
    }
    Ok((mailbox, client_id, tenant_id))
}

fn auth_client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?)
}

fn validate_graph_url(url: &Url) -> GraphResult<()> {
    if url.scheme() != "https"
        || url.host_str() != Some("graph.microsoft.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || !url.path().starts_with("/v1.0/me")
    {
        return Err(GraphError {
            status: None,
            retry_after: 60,
            message: "Refusing a Microsoft URL outside the registered mailbox API".into(),
        });
    }
    Ok(())
}

fn path_id(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_accounts_and_graph_urls() {
        let account = validate_account(
            "RAD@EXAMPLE.ORG",
            "9ca3f2a1-6b23-43a8-a342-28aa257c2e8c",
            "7bcc5e4c-57b7-49ef-aecc-43142b99b443",
        )
        .expect("account");
        assert_eq!(account.0, "rad@example.org");
        assert!(
            validate_graph_url(
                &Url::parse("https://graph.microsoft.com/v1.0/me/messages").expect("URL")
            )
            .is_ok()
        );
        assert!(
            validate_graph_url(
                &Url::parse("https://graph.microsoft.com.evil.test/v1.0/me/messages").expect("URL")
            )
            .is_err()
        );
    }

    #[test]
    fn encodes_message_ids_as_one_path_component() {
        assert_eq!(path_id("a/b@example"), "a%2Fb%40example");
    }
}
