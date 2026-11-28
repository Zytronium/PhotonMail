//! Google OAuth for Gmail accounts using the installed desktop-app flow.

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use reqwest::Client;
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use url::Url;

const AUTHORIZATION_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const USERINFO_ENDPOINT: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const GMAIL_SCOPE: &str = "https://mail.google.com/";
const GOOGLE_CLIENT_ID: &str =
    "45885468945-pm0a0j1h9s4ug6fercqk4val07splujs.apps.googleusercontent.com";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GoogleAccount {
    pub email: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct UserInfo {
    email: String,
    email_verified: Option<bool>,
}

/// Run browser authorization off the Freya UI executor and return its result asynchronously.
pub fn sign_in_in_background() -> async_channel::Receiver<Result<GoogleAccount, String>> {
    let (tx, rx) = async_channel::bounded(1);
    let worker_tx = tx.clone();
    let spawn_result = std::thread::Builder::new()
        .name("photon-mail-google-oauth".into())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("Couldn't start Google sign-in: {error}"))
                .and_then(|runtime| runtime.block_on(authorize()));
            let _ = worker_tx.send_blocking(result);
        });
    if let Err(error) = spawn_result {
        let _ = tx.send_blocking(Err(format!("Couldn't start Google sign-in: {error}")));
    }
    rx
}

async fn authorize() -> Result<GoogleAccount, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("Couldn't start the local Google sign-in callback: {error}"))?;
    let redirect_uri = format!(
        "http://{}",
        listener.local_addr().map_err(|error| error.to_string())?
    );
    let state = random_url_token();
    let verifier = random_url_token();
    let challenge = pkce_challenge(&verifier);
    let authorization_url = authorization_url(GOOGLE_CLIENT_ID, &redirect_uri, &state, &challenge)?;

    open::that(authorization_url.as_str())
        .map_err(|error| format!("Couldn't open a browser for Google sign-in: {error}"))?;

    let code = timeout(
        CALLBACK_TIMEOUT,
        wait_for_authorization_code(listener, &state),
    )
    .await
    .map_err(|_| "Google sign-in timed out. Please try again.".to_string())??;

    let http = Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|error| format!("Couldn't prepare Google sign-in: {error}"))?;
    let token = exchange_code(&http, GOOGLE_CLIENT_ID, &code, &verifier, &redirect_uri).await?;
    let refresh_token = token.refresh_token.ok_or_else(|| {
        "Google didn't return a refresh token. Remove Photon Mail from your Google Account's third-party connections and try again.".to_string()
    })?;
    let user_info = fetch_user_info(&http, &token.access_token).await?;
    if user_info.email.trim().is_empty() || user_info.email_verified != Some(true) {
        return Err(
            "Google did not confirm a verified email address for this account.".to_string(),
        );
    }

    Ok(GoogleAccount {
        email: user_info.email,
        refresh_token,
    })
}

fn random_url_token() -> String {
    let mut bytes = [0; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn authorization_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<Url, String> {
    let mut url = Url::parse(AUTHORIZATION_ENDPOINT)
        .map_err(|error| format!("Invalid Google authorization endpoint: {error}"))?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &format!("openid email {GMAIL_SCOPE}"))
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url)
}

async fn wait_for_authorization_code(
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("Google sign-in callback failed: {error}"))?;
        let request = read_callback_request(&mut stream).await?;
        let Some(target) = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
        else {
            write_callback_response(&mut stream, 400, "Invalid sign-in callback.").await;
            continue;
        };
        if !target.starts_with('/') || target.starts_with("//") {
            write_callback_response(&mut stream, 400, "Invalid sign-in callback.").await;
            continue;
        }
        let callback = Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|error| format!("Invalid Google sign-in callback: {error}"))?;
        if callback.path() != "/" {
            write_callback_response(&mut stream, 404, "Not found.").await;
            continue;
        }

        match callback_result(&callback, expected_state) {
            Ok(code) => {
                write_callback_response(
                    &mut stream,
                    200,
                    "Google sign-in complete. You can close this browser window and return to Photon Mail.",
                )
                .await;
                return Ok(code);
            }
            Err(error) if error == "Invalid OAuth state." => {
                write_callback_response(&mut stream, 400, "Invalid sign-in callback.").await;
            }
            Err(error) => {
                write_callback_response(&mut stream, 400, "Google sign-in was not completed.")
                    .await;
                return Err(error);
            }
        }
    }
}

async fn read_callback_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0; 512];
    loop {
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| format!("Couldn't read the Google sign-in callback: {error}"))?;
        if count == 0 {
            return Err("Google closed the sign-in callback without a response.".to_string());
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() >= 8192 {
            return Err("Google sign-in callback headers were too large.".to_string());
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

fn callback_result(callback: &Url, expected_state: &str) -> Result<String, String> {
    let mut state = None;
    let mut code = None;
    let mut oauth_error = None;
    for (key, value) in callback.query_pairs() {
        match key.as_ref() {
            "state" => state = Some(value.into_owned()),
            "code" => code = Some(value.into_owned()),
            "error" => oauth_error = Some(value.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Err("Invalid OAuth state.".to_string());
    }
    if let Some(error) = oauth_error {
        return Err(if error == "access_denied" {
            "Google sign-in was cancelled.".to_string()
        } else {
            format!("Google sign-in failed: {error}")
        });
    }
    code.filter(|value| !value.is_empty())
        .ok_or_else(|| "Google did not return an authorization code.".to_string())
}

async fn write_callback_response(stream: &mut TcpStream, status: u16, text: &str) {
    let status_text = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body =
        format!("<!doctype html><meta charset=\"utf-8\"><title>Photon Mail</title><p>{text}</p>");
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn exchange_code(
    http: &Client,
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let response = post_form(
        http,
        &[
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ],
    )
    .await?;
    parse_token_response(response, "Google authorization-code exchange").await
}

async fn fetch_user_info(http: &Client, access_token: &str) -> Result<UserInfo, String> {
    let response = http
        .get(USERINFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| format!("Couldn't get the Google account email: {error}"))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("Couldn't read the Google account response: {error}"))?;
    if !status.is_success() {
        return Err(format!("Google account lookup failed ({status})."));
    }
    serde_json::from_slice(&body)
        .map_err(|error| format!("Google returned an invalid account response: {error}"))
}

/// Resolve an account's stored secret to a fresh OAuth access token when needed.
/// Plain passwords and legacy manually supplied access tokens are left unchanged.
pub async fn access_token(secret: &str) -> Result<Option<String>, String> {
    if let Some(refresh_token) = secret.strip_prefix("oauth2-refresh:") {
        if refresh_token.is_empty() {
            return Err("OAuth2 refresh token is empty.".to_string());
        }
        let http = Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|error| format!("Couldn't prepare Google token refresh: {error}"))?;
        let response = post_form(
            &http,
            &[
                ("client_id", GOOGLE_CLIENT_ID),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ],
        )
        .await?;
        return parse_token_response(response, "Google access-token refresh")
            .await
            .map(|token| Some(token.access_token));
    }
    if let Some(token) = secret.strip_prefix("oauth2:") {
        if token.is_empty() {
            return Err("OAuth2 access token is empty.".to_string());
        }
        return Ok(Some(token.to_string()));
    }
    Ok(None)
}

async fn post_form(http: &Client, fields: &[(&str, &str)]) -> Result<reqwest::Response, String> {
    let body = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in fields {
            form.append_pair(key, value);
        }
        form.finish()
    };
    http.post(TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|error| format!("Couldn't contact Google's token service: {error}"))
}

async fn parse_token_response(
    response: reqwest::Response,
    operation: &str,
) -> Result<TokenResponse, String> {
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("Couldn't read Google's token response: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "{operation} failed ({status}); reconnect the Google account if access was revoked."
        ));
    }
    let token: TokenResponse = serde_json::from_slice(&body)
        .map_err(|error| format!("Google returned an invalid token response: {error}"))?;
    if token.access_token.is_empty() {
        return Err("Google returned an empty access token.".to_string());
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn callback_requires_the_matching_state_and_authorization_code() {
        let callback = Url::parse("http://127.0.0.1/?state=secure&code=abc").unwrap();
        assert_eq!(callback_result(&callback, "secure").unwrap(), "abc");
        assert_eq!(
            callback_result(&callback, "wrong").unwrap_err(),
            "Invalid OAuth state."
        );

        let denied = Url::parse("http://127.0.0.1/?state=secure&error=access_denied").unwrap();
        assert_eq!(
            callback_result(&denied, "secure").unwrap_err(),
            "Google sign-in was cancelled."
        );
    }

    #[test]
    fn authorization_url_requests_gmail_access_offline_with_pkce() {
        let url =
            authorization_url("client-id", "http://127.0.0.1:4321", "state", "challenge").unwrap();
        let query = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            query.get("client_id").map(String::as_str),
            Some("client-id")
        );
        assert_eq!(
            query.get("scope").map(String::as_str),
            Some("openid email https://mail.google.com/")
        );
        assert_eq!(
            query.get("access_type").map(String::as_str),
            Some("offline")
        );
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
    }
}
