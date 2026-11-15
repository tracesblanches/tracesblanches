//! Strava OAuth token exchange (S1): the client secret stays on the server.
//!
//! Strava has no PKCE and requires `client_secret` to exchange an authorization code and
//! to refresh a token (L5, `docs/licences/l5-strava.md`). The app opens Strava's mobile
//! authorization page with the public `client_id` (from `/v1/strava/config`), Strava
//! redirects to `/v1/strava/callback`, which hands the code to the app (custom scheme), and
//! the app posts the code here. Responses of Strava are passed through as they are; nothing
//! is stored or logged (API Policy 5.5, 5.7, 6.2: no Strava data kept).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::http::{Reply, Req};
use crate::util::{base64, form, form_encode, parse_query};

/// Strava settings.
#[derive(Clone)]
pub struct StravaConfig {
    /// Public client id of the Strava API application.
    pub client_id: String,
    /// Client secret (Kubernetes Secret, never in the app).
    pub client_secret: String,
    /// `https://www.strava.com` (a fake server in tests).
    pub base_url: String,
    /// Scopes the app asks for.
    pub scope: String,
    /// Where `/v1/strava/callback` sends the browser: the app's custom scheme.
    pub app_redirect: String,
    /// Public URL of this server (redirect URI given to Strava).
    pub public_url: String,
}

impl std::fmt::Debug for StravaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StravaConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"***")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// What the app needs to start the authorization (`GET /v1/strava/config`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicConfig {
    /// Whether the exchange is configured on this server.
    pub configured: bool,
    /// Client id.
    pub client_id: Option<String>,
    /// Strava's mobile authorization page.
    pub authorize_url: String,
    /// Redirect URI to pass to Strava (its domain is the app's "Authorization Callback
    /// Domain").
    pub redirect_uri: String,
    /// Scopes to ask for.
    pub scope: String,
}

/// The token exchange.
pub struct Strava {
    cfg: StravaConfig,
    http: reqwest::blocking::Client,
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

#[derive(Deserialize)]
struct RefreshBody {
    refresh_token: String,
}

#[derive(Deserialize)]
struct RevokeBody {
    token: String,
    #[serde(default)]
    token_type_hint: Option<String>,
}

impl Strava {
    /// A client for Strava with these settings.
    pub fn new(cfg: StravaConfig) -> Result<Self> {
        let mut tls = Arc::unwrap_or_clone(crate::alerts::mail::tls_config()?);
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("skimap-server/", env!("CARGO_PKG_VERSION")))
            .tls_backend_preconfigured(tls)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow!("HTTP client: {e}"))?;
        Ok(Self { cfg, http })
    }

    /// What `/v1/strava/config` answers.
    pub fn public_config(cfg: Option<&StravaConfig>, public_url: &str) -> PublicConfig {
        PublicConfig {
            configured: cfg.is_some(),
            client_id: cfg.map(|c| c.client_id.clone()),
            authorize_url: "https://www.strava.com/oauth/mobile/authorize".into(),
            redirect_uri: format!("{}/v1/strava/callback", public_url.trim_end_matches('/')),
            scope: cfg.map_or_else(|| "activity:write".to_owned(), |c| c.scope.clone()),
        }
    }

    fn post(&self, path: &str, body: String, basic: bool) -> Reply {
        let url = format!("{}{path}", self.cfg.base_url.trim_end_matches('/'));
        let mut rq = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(body);
        if basic {
            let cred =
                base64(format!("{}:{}", self.cfg.client_id, self.cfg.client_secret).as_bytes());
            rq = rq.header("Authorization", format!("Basic {cred}"));
        }
        match rq.send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let ct = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/json")
                    .to_owned();
                match resp.bytes() {
                    Ok(b) => {
                        tracing::info!(path, status, "strava");
                        // Strava's own status (200, 400, 401...) and body, unchanged.
                        Reply::bytes(status, &ct, b.to_vec()).with("Cache-Control", "no-store")
                    }
                    Err(e) => Reply::error(
                        502,
                        "strava_unreachable",
                        format!("reading Strava's answer: {}", e.without_url()),
                    ),
                }
            }
            Err(e) => {
                let e = e.without_url();
                tracing::warn!(path, error = %e, "strava unreachable");
                Reply::error(
                    502,
                    "strava_unreachable",
                    format!("Strava did not answer: {e}"),
                )
            }
        }
    }

    /// `POST /v1/strava/token` `{"code": "..."}`: authorization code → tokens.
    pub fn exchange(&self, req: &Req) -> Reply {
        let Some(b) = parse_body::<CodeBody>(req).filter(|b| token_like(&b.code)) else {
            return Reply::error(400, "bad_request", "body: {\"code\": \"...\"}");
        };
        self.post(
            "/oauth/token",
            form(&[
                ("client_id", &self.cfg.client_id),
                ("client_secret", &self.cfg.client_secret),
                ("code", &b.code),
                ("grant_type", "authorization_code"),
            ]),
            false,
        )
    }

    /// `POST /v1/strava/refresh` `{"refresh_token": "..."}`: new short-lived access token.
    pub fn refresh(&self, req: &Req) -> Reply {
        let Some(b) = parse_body::<RefreshBody>(req).filter(|b| token_like(&b.refresh_token))
        else {
            return Reply::error(400, "bad_request", "body: {\"refresh_token\": \"...\"}");
        };
        self.post(
            "/oauth/token",
            form(&[
                ("client_id", &self.cfg.client_id),
                ("client_secret", &self.cfg.client_secret),
                ("grant_type", "refresh_token"),
                ("refresh_token", &b.refresh_token),
            ]),
            false,
        )
    }

    /// `POST /v1/strava/revoke` `{"token": "...", "token_type_hint": "refresh_token"}`:
    /// disconnects the app (Strava's `/oauth/revoke`, HTTP Basic with the client secret).
    pub fn revoke(&self, req: &Req) -> Reply {
        let Some(b) = parse_body::<RevokeBody>(req).filter(|b| token_like(&b.token)) else {
            return Reply::error(400, "bad_request", "body: {\"token\": \"...\"}");
        };
        let mut pairs = vec![("token", b.token.as_str())];
        if let Some(h) = b
            .token_type_hint
            .as_deref()
            .filter(|h| *h == "access_token" || *h == "refresh_token")
        {
            pairs.push(("token_type_hint", h));
        }
        self.post("/oauth/revoke", form(&pairs), true)
    }

    /// `GET /v1/strava/callback?code=…&scope=…&state=…` (or `error=…`): Strava's redirect,
    /// handed to the app's custom scheme with the same parameters.
    pub fn callback(&self, req: &Req) -> Reply {
        callback(&self.cfg.app_redirect, req)
    }
}

/// The callback redirect, also when Strava is not configured (tests).
pub fn callback(app_redirect: &str, req: &Req) -> Reply {
    let kept: Vec<(String, String)> = parse_query(&req.query)
        .into_iter()
        .filter(|(k, v)| {
            matches!(k.as_str(), "code" | "scope" | "state" | "error") && v.len() <= 512
        })
        .collect();
    let q = kept
        .iter()
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let sep = if app_redirect.contains('?') { '&' } else { '?' };
    let target = format!("{app_redirect}{sep}{q}");
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>skimap</title>\
         <p>Retour vers l'application skimap… <a href=\"{0}\">Continuer</a></p>",
        target.replace('&', "&amp;").replace('"', "&quot;")
    );
    Reply::html(302, &html)
        .with("Location", target)
        .with("Cache-Control", "no-store")
        .with("Referrer-Policy", "no-referrer")
}

fn parse_body<T: serde::de::DeserializeOwned>(req: &Req) -> Option<T> {
    serde_json::from_slice(&req.body).ok()
}

/// Codes and tokens of Strava are short alphanumeric strings.
fn token_like(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_keeps_only_oauth_parameters() {
        let r = callback(
            "skimap://strava/callback",
            &Req::new(
                "GET",
                "/v1/strava/callback?state=x1&code=abc123&scope=read,activity:write&evil=1",
            ),
        );
        assert_eq!(r.status, 302);
        assert_eq!(
            r.header("location"),
            Some("skimap://strava/callback?state=x1&code=abc123&scope=read%2Cactivity%3Awrite")
        );
        let r = callback(
            "skimap://s?x=1",
            &Req::new("GET", "/cb?error=access_denied"),
        );
        assert_eq!(
            r.header("location"),
            Some("skimap://s?x=1&error=access_denied")
        );
    }

    #[test]
    fn tokens() {
        assert!(token_like("a1b2c3"));
        assert!(!token_like("a b") && !token_like("") && !token_like("a&b"));
    }
}
