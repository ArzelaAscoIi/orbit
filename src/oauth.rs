//! Sign in with Linear: OAuth 2.0 authorization code with PKCE.
//!
//! No client secret: with PKCE Linear needs only the public client id, for
//! the code exchange and for every refresh. The browser comes back to a
//! one-shot listener on the registered loopback redirect.
//!
//! The result is a `Tokens` JSON string, stored and passed around exactly
//! where a `lin_api_` key would be. `linear::Client` tells the two apart and
//! refreshes the access token (valid 24h) before it runs out.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};

/// The Linear OAuth app sign-in uses: the profile's (`linear.oauthClientId`),
/// since an OAuth app belongs to one workspace. Without one, API key only.
pub fn client_id() -> Option<String> {
    Some(
        crate::profile::get()
            .linear
            .oauth_client_id
            .trim()
            .to_owned(),
    )
    .filter(|c| !c.is_empty())
}
const PORT: u16 = 53682;
const REDIRECT: &str = "http://localhost:53682/callback";
const AUTHORIZE: &str = "https://linear.app/oauth/authorize";
const TOKEN_URL: &str = "https://api.linear.app/oauth/token";
const SCOPE: &str = "read,write";
/// How long the browser gets before the listener gives the port back.
const WAIT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    /// Unix seconds.
    pub expires: u64,
    /// The OAuth app they were issued to, which must refresh them; older
    /// tokens have none and use the profile's.
    #[serde(default)]
    pub client: String,
}

impl Tokens {
    pub fn parse(stored: &str) -> Option<Self> {
        serde_json::from_str(stored.trim()).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Refresh five minutes early so no request races the expiry.
    pub fn stale(&self) -> bool {
        now() + 300 >= self.expires
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Starts the browser sign-in on its own thread. The receiver gets the
/// `Tokens` JSON, or a message for the login screen.
pub fn sign_in(ctx: egui::Context) -> mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("oauth".into())
        .spawn(move || {
            let result = run(&ctx).map(|t| t.to_json()).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        })
        .expect("oauth thread");
    rx
}

fn run(ctx: &egui::Context) -> Result<Tokens> {
    // ponytail: IPv4 only; browsers fall back from ::1 when it refuses.
    let listener = TcpListener::bind(("127.0.0.1", PORT))
        .with_context(|| format!("port {PORT} is busy; try again in a few minutes"))?;
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let client = client_id().ok_or_else(|| anyhow!("no Linear OAuth app: sign in with a key"))?;
    let state = uuid::Uuid::new_v4().simple().to_string();
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE,
        [
            ("client_id", client.as_str()),
            ("redirect_uri", REDIRECT),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("state", &state),
            ("code_challenge", &challenge(&verifier)),
            ("code_challenge_method", "S256"),
        ],
    )?;
    ctx.open_url(egui::OpenUrl::new_tab(url));
    ctx.request_repaint();

    let code = wait_for_code(&listener, &state)?;
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", REDIRECT),
        ("client_id", client.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(token(&crate::http(), TOKEN_URL, &form, "", &client))
}

/// A new access token for `tokens`. Linear may rotate the refresh token;
/// the old one is kept when it does not.
pub async fn refresh(http: &reqwest::Client, url: &str, tokens: &Tokens) -> Result<Tokens> {
    let client = Some(tokens.client.clone())
        .filter(|c| !c.is_empty())
        .or_else(client_id)
        .ok_or_else(|| anyhow!("no Linear OAuth app to refresh with"))?;
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", tokens.refresh.as_str()),
        ("client_id", client.as_str()),
    ];
    token(http, url, &form, &tokens.refresh, &client).await
}

async fn token(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
    old_refresh: &str,
    client: &str,
) -> Result<Tokens> {
    #[derive(Deserialize)]
    struct Answer {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: u64,
    }
    let response = http
        .post(url)
        .form(form)
        .send()
        .await
        .map_err(|error| anyhow!("network error: {}", error.without_url()))?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let why = body["error_description"]
            .as_str()
            .or(body["error"].as_str())
            .unwrap_or("no reason given");
        bail!("Linear refused the sign-in ({status}): {why}");
    }
    let answer: Answer = serde_json::from_value(body)?;
    Ok(Tokens {
        access: answer.access_token,
        refresh: answer
            .refresh_token
            .unwrap_or_else(|| old_refresh.to_owned()),
        expires: now() + answer.expires_in,
        client: client.to_owned(),
    })
}

/// BASE64URL(SHA256(verifier)), RFC 7636.
fn challenge(verifier: &str) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

/// Answers requests on the loopback port until the browser brings the code
/// back, or `WAIT` runs out.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let started = Instant::now();
    loop {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() > WAIT {
                    bail!("no answer from the browser; sign in again");
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        // macOS hands out accepted sockets non-blocking like the listener.
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let answer = callback(&line, state);
        let page = match &answer {
            Some(Ok(_)) => "Signed in. You can close this tab and go back to the app.".to_owned(),
            Some(Err(e)) => format!("Sign-in failed: {e}"),
            None => "Not found".to_owned(),
        };
        let status = if answer.is_some() {
            "200 OK"
        } else {
            "404 Not Found"
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        if let Some(answer) = answer {
            return answer;
        }
    }
}

/// The code in a request line like `GET /callback?code=..&state=.. HTTP/1.1`.
/// `None` when the request is not the callback (a favicon, say).
fn callback(request_line: &str, state: &str) -> Option<Result<String>> {
    let path = request_line.split_whitespace().nth(1)?;
    let url = reqwest::Url::parse(&format!("http://localhost{path}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let get = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    Some(if let Some(error) = get("error") {
        Err(anyhow!("Linear said {error}"))
    } else if get("state").as_deref() != Some(state) {
        Err(anyhow!("the answer did not match this sign-in; try again"))
    } else {
        get("code").ok_or_else(|| anyhow!("Linear sent no code"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_rfc_7636() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn callback_takes_the_code_only_with_our_state() {
        let ok = callback("GET /callback?code=abc&state=s1 HTTP/1.1\r\n", "s1");
        assert_eq!(ok.unwrap().unwrap(), "abc");
        assert!(
            callback("GET /callback?code=abc&state=zz HTTP/1.1", "s1")
                .unwrap()
                .is_err()
        );
        assert!(
            callback("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1")
                .unwrap()
                .is_err()
        );
        assert!(callback("GET /favicon.ico HTTP/1.1", "s1").is_none());
    }

    #[test]
    fn stored_tokens_round_trip_and_keys_are_not_tokens() {
        let t = Tokens {
            access: "a".into(),
            refresh: "r".into(),
            expires: 1,
            client: String::new(),
        };
        assert_eq!(Tokens::parse(&t.to_json()), Some(t.clone()));
        assert!(t.stale());
        assert_eq!(Tokens::parse("lin_api_abc"), None);
    }
}
