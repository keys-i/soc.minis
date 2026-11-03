use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use rsa::{
    BigUint, Pkcs1v15Sign, RsaPublicKey,
    sha2::{Digest, Sha256},
};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::error::{Result, invalid};

const JWKS_LIMIT: usize = 128 * 1024;
const JWT_LIMIT: usize = 16 * 1024;
const CLOCK_SKEW_SECONDS: i64 = 30;
const JWKS_TTL: Duration = Duration::from_secs(60 * 60);
const JWKS_STALE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MISSING_KEY_RETRY: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub enum Access {
    Local { owner: Arc<str> },
    Hosted { verifier: Verifier },
}

#[derive(Debug, PartialEq, Eq)]
pub enum AccessError {
    Invalid,
    Unavailable,
}

#[derive(Clone)]
pub struct Verifier {
    client: Client,
    issuer: Arc<str>,
    audience: Arc<str>,
    jwks_url: Url,
    cache: Arc<Mutex<KeyCache>>,
}

struct KeyCache {
    keys: HashMap<String, RsaPublicKey>,
    expires: Instant,
    stale_until: Instant,
    next_missing_refresh: Instant,
}

impl KeyCache {
    fn stale_key(&self, kid: &str, now: Instant) -> Option<RsaPublicKey> {
        (now < self.stale_until)
            .then(|| self.keys.get(kid).cloned())
            .flatten()
    }
}

#[derive(Deserialize)]
struct KeySet {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    #[serde(default)]
    alg: String,
    #[serde(default, rename = "use")]
    usage: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

#[derive(Clone, Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    exp: i64,
    nbf: Option<i64>,
    iat: Option<i64>,
    email: String,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Access {
    pub fn from_env(local: bool) -> Result<Self> {
        let hosted = ["SOC_ACCESS_ISSUER", "SOC_ACCESS_AUDIENCE"]
            .iter()
            .any(|name| env::var_os(name).is_some());
        if !hosted {
            if local {
                return Self::local(env::var("SOC_DEV_USER").ok());
            }
            return Err(invalid(
                "Configure Cloudflare Access before hosting SOC Minis",
            ));
        }
        let issuer = required("SOC_ACCESS_ISSUER")?;
        let audience = required("SOC_ACCESS_AUDIENCE")?;
        Ok(Self::Hosted {
            verifier: Verifier::new(&issuer, &audience)?,
        })
    }

    pub fn local(configured: Option<String>) -> Result<Self> {
        let owner = configured.unwrap_or_else(|| "local@localhost".into());
        let owner = email(&owner, true).ok_or_else(|| {
            invalid("SOC_DEV_USER must be one fixed email address for this local server")
        })?;
        Ok(Self::Local {
            owner: Arc::from(owner),
        })
    }

    pub fn hosted(&self) -> bool {
        matches!(self, Self::Hosted { .. })
    }

    pub async fn owner(&self, headers: &HeaderMap) -> std::result::Result<String, AccessError> {
        match self {
            Self::Local { owner } => Ok(owner.to_string()),
            Self::Hosted { verifier } => {
                let token = headers
                    .get("cf-access-jwt-assertion")
                    .and_then(|value| value.to_str().ok())
                    .ok_or(AccessError::Invalid)?;
                verifier.verify(token).await
            }
        }
    }
}

impl Verifier {
    fn new(issuer: &str, audience: &str) -> Result<Self> {
        let issuer = issuer.trim_end_matches('/');
        let parsed = Url::parse(issuer).map_err(|_| invalid("SOC_ACCESS_ISSUER is not a URL"))?;
        if parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || !parsed
                .host_str()
                .is_some_and(|host| host.ends_with(".cloudflareaccess.com"))
        {
            return Err(invalid(
                "SOC_ACCESS_ISSUER must be the HTTPS Cloudflare Access team domain",
            ));
        }
        let audience = audience.trim();
        if audience.is_empty() || audience.len() > 256 || !audience.is_ascii() {
            return Err(invalid("SOC_ACCESS_AUDIENCE is invalid"));
        }
        let jwks_url = parsed
            .join("/cdn-cgi/access/certs")
            .map_err(|_| invalid("Cannot build the Cloudflare Access certificate URL"))?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .https_only(true)
            .build()
            .map_err(crate::error::SocError::from)?;
        let now = Instant::now();
        Ok(Self {
            client,
            issuer: Arc::from(issuer),
            audience: Arc::from(audience),
            jwks_url,
            cache: Arc::new(Mutex::new(KeyCache {
                keys: HashMap::new(),
                expires: now,
                stale_until: now,
                next_missing_refresh: now,
            })),
        })
    }

    async fn verify(&self, token: &str) -> std::result::Result<String, AccessError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AccessError::Unavailable)?
            .as_secs()
            .try_into()
            .map_err(|_| AccessError::Unavailable)?;
        self.verify_at(token, now).await
    }

    async fn verify_at(&self, token: &str, now: i64) -> std::result::Result<String, AccessError> {
        if token.len() > JWT_LIMIT {
            return Err(AccessError::Invalid);
        }
        let mut parts = token.split('.');
        let encoded_header = parts.next().ok_or(AccessError::Invalid)?;
        let encoded_claims = parts.next().ok_or(AccessError::Invalid)?;
        let encoded_signature = parts.next().ok_or(AccessError::Invalid)?;
        if parts.next().is_some() || encoded_signature.is_empty() {
            return Err(AccessError::Invalid);
        }
        let header: Header = decode_json(encoded_header)?;
        if header.alg != "RS256" || header.kid.is_empty() || header.kid.len() > 128 {
            return Err(AccessError::Invalid);
        }
        let key = self.key(&header.kid).await?;
        let signature = URL_SAFE_NO_PAD
            .decode(encoded_signature)
            .map_err(|_| AccessError::Invalid)?;
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        let digest = Sha256::digest(signing_input.as_bytes());
        key.verify(Pkcs1v15Sign::new::<Sha256>(), &digest, &signature)
            .map_err(|_| AccessError::Invalid)?;
        let claims: Claims = decode_json(encoded_claims)?;
        validate_claims(&claims, &self.issuer, &self.audience, now)
    }

    async fn key(&self, kid: &str) -> std::result::Result<RsaPublicKey, AccessError> {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        let stale = cache.stale_key(kid, now);
        if now < cache.expires
            && let Some(key) = stale
        {
            return Ok(key);
        }
        if now < cache.next_missing_refresh {
            return stale.ok_or(if cache.keys.is_empty() {
                AccessError::Unavailable
            } else {
                AccessError::Invalid
            });
        }
        match self.fetch_keys().await {
            Ok(keys) => {
                cache.keys = keys;
                cache.expires = now + JWKS_TTL;
                cache.stale_until = now + JWKS_STALE_TTL;
                cache.next_missing_refresh = now + MISSING_KEY_RETRY;
                cache.keys.get(kid).cloned().ok_or(AccessError::Invalid)
            }
            Err(error) => {
                cache.next_missing_refresh = now + MISSING_KEY_RETRY;
                stale.ok_or(error)
            }
        }
    }

    async fn fetch_keys(&self) -> std::result::Result<HashMap<String, RsaPublicKey>, AccessError> {
        let response = self
            .client
            .get(self.jwks_url.clone())
            .send()
            .await
            .map_err(|_| AccessError::Unavailable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > JWKS_LIMIT as u64)
        {
            return Err(AccessError::Unavailable);
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| AccessError::Unavailable)?;
        if body.len() > JWKS_LIMIT {
            return Err(AccessError::Unavailable);
        }
        let set: KeySet = serde_json::from_slice(&body).map_err(|_| AccessError::Unavailable)?;
        if set.keys.is_empty() || set.keys.len() > 32 {
            return Err(AccessError::Unavailable);
        }
        let keys = set
            .keys
            .into_iter()
            .filter_map(jwk)
            .collect::<HashMap<_, _>>();
        if keys.is_empty() {
            return Err(AccessError::Unavailable);
        }
        Ok(keys)
    }
}

fn decode_json<T: for<'de> Deserialize<'de>>(encoded: &str) -> std::result::Result<T, AccessError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| AccessError::Invalid)?;
    serde_json::from_slice(&bytes).map_err(|_| AccessError::Invalid)
}

fn jwk(value: Jwk) -> Option<(String, RsaPublicKey)> {
    if value.kid.is_empty()
        || value.kid.len() > 128
        || value.kty != "RSA"
        || (!value.alg.is_empty() && value.alg != "RS256")
        || (!value.usage.is_empty() && value.usage != "sig")
    {
        return None;
    }
    let modulus = URL_SAFE_NO_PAD.decode(value.n).ok()?;
    let exponent = URL_SAFE_NO_PAD.decode(value.e).ok()?;
    if !(256..=1024).contains(&modulus.len()) || exponent.is_empty() || exponent.len() > 8 {
        return None;
    }
    let key = RsaPublicKey::new(
        BigUint::from_bytes_be(&modulus),
        BigUint::from_bytes_be(&exponent),
    )
    .ok()?;
    Some((value.kid, key))
}

fn validate_claims(
    claims: &Claims,
    issuer: &str,
    audience: &str,
    now: i64,
) -> std::result::Result<String, AccessError> {
    let audience_matches = match &claims.aud {
        Audience::One(value) => value == audience,
        Audience::Many(values) => values.iter().any(|value| value == audience),
    };
    if claims.iss != issuer
        || !audience_matches
        || claims.exp <= now - CLOCK_SKEW_SECONDS
        || claims
            .nbf
            .is_some_and(|not_before| not_before > now + CLOCK_SKEW_SECONDS)
        || claims
            .iat
            .is_some_and(|issued| issued > now + CLOCK_SKEW_SECONDS)
    {
        return Err(AccessError::Invalid);
    }
    email(&claims.email, false).ok_or(AccessError::Invalid)
}

fn email(value: &str, local: bool) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() > 254
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return None;
    }
    let (name, domain) = value.split_once('@')?;
    if name.is_empty()
        || name.len() > 64
        || domain.is_empty()
        || domain.contains('@')
        || name.starts_with('.')
        || name.ends_with('.')
        || name.contains("..")
        || (!local && !domain.contains('.'))
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return None;
    }
    Some(value)
}

fn required(name: &str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(format!("Set {name} before hosting SOC Minis")))
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use rand::thread_rng;
    use rsa::{
        RsaPrivateKey,
        pkcs1v15::SigningKey,
        sha2::Sha256,
        signature::{SignatureEncoding, Signer},
    };

    use super::*;

    #[test]
    fn rejects_wrong_or_stale_access_claims() {
        let base = Claims {
            iss: "https://team.cloudflareaccess.com".into(),
            aud: Audience::One("soc-audience".into()),
            exp: 1_100,
            nbf: Some(900),
            iat: Some(900),
            email: "Exec@Example.org".into(),
        };
        assert_eq!(
            validate_claims(
                &base,
                "https://team.cloudflareaccess.com",
                "soc-audience",
                1_000
            ),
            Ok("exec@example.org".into())
        );
        for claims in [
            Claims {
                iss: "https://other.cloudflareaccess.com".into(),
                ..base.clone()
            },
            Claims {
                iss: "https://team.cloudflareaccess.com/".into(),
                ..base.clone()
            },
            Claims {
                aud: Audience::One("other".into()),
                ..base.clone()
            },
            Claims {
                exp: 900,
                ..base.clone()
            },
            Claims {
                nbf: Some(1_100),
                ..base.clone()
            },
            Claims {
                iat: Some(1_100),
                ..base.clone()
            },
            Claims {
                email: "not-an-email".into(),
                ..base.clone()
            },
        ] {
            assert_eq!(
                validate_claims(
                    &claims,
                    "https://team.cloudflareaccess.com",
                    "soc-audience",
                    1_000
                ),
                Err(AccessError::Invalid)
            );
        }
    }

    #[tokio::test]
    async fn verifies_rs256_before_accepting_claims() {
        let private = RsaPrivateKey::new(&mut thread_rng(), 2048).expect("test key");
        let public = RsaPublicKey::from(&private);
        let signer = SigningKey::<Sha256>::new(private);
        let now: i64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs()
            .try_into()
            .expect("test timestamp");
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"test"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::json!({
                "iss": "https://team.cloudflareaccess.com",
                "aud": ["soc-audience"],
                "exp": now + 100,
                "nbf": now - 100,
                "iat": now - 100,
                "email": "exec@example.org",
            })
            .to_string(),
        );
        let message = format!("{header}.{claims}");
        let signature = URL_SAFE_NO_PAD.encode(signer.sign(message.as_bytes()).to_bytes());
        let cached_at = Instant::now();
        let verifier = Verifier {
            client: Client::new(),
            issuer: Arc::from("https://team.cloudflareaccess.com"),
            audience: Arc::from("soc-audience"),
            jwks_url: Url::parse("https://team.cloudflareaccess.com/cdn-cgi/access/certs")
                .expect("JWKS URL"),
            cache: Arc::new(Mutex::new(KeyCache {
                keys: HashMap::from([("test".into(), public)]),
                expires: cached_at + Duration::from_secs(60),
                stale_until: cached_at + Duration::from_secs(120),
                next_missing_refresh: cached_at + Duration::from_secs(60),
            })),
        };
        let token = format!("{message}.{signature}");
        assert_eq!(
            verifier.verify_at(&token, now).await,
            Ok("exec@example.org".into())
        );
        let access = Access::Hosted {
            verifier: verifier.clone(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "cf-access-jwt-assertion",
            token.parse().expect("JWT header"),
        );
        headers.insert(
            "cf-access-authenticated-user-email",
            "attacker@example.org".parse().expect("spoofed header"),
        );
        assert_eq!(access.owner(&headers).await, Ok("exec@example.org".into()));
        let mut changed = token.clone();
        let replacement = if changed.ends_with('A') { "B" } else { "A" };
        changed.replace_range(changed.len() - 1.., replacement);
        assert_eq!(
            verifier.verify_at(&changed, now).await,
            Err(AccessError::Invalid)
        );
        let cache = verifier.cache.lock().await;
        assert!(
            cache
                .stale_key("test", cached_at + Duration::from_secs(119))
                .is_some()
        );
        assert!(
            cache
                .stale_key("test", cached_at + Duration::from_secs(120))
                .is_none()
        );
    }

    #[tokio::test]
    async fn local_identity_ignores_spoofable_headers() {
        let access = Access::local(Some("owner@example.org".into())).expect("local access");
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-soc-user",
            "attacker@example.org".parse().expect("header"),
        );
        headers.insert(
            "cf-access-authenticated-user-email",
            "attacker@example.org".parse().expect("header"),
        );
        assert_eq!(access.owner(&headers).await, Ok("owner@example.org".into()));
    }
}
