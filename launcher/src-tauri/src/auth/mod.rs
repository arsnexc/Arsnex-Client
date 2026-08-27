//! Arsex Client — Microsoft authentication.
//!
//! Full five-leg chain: MSA -> Xbox Live -> XSTS -> Minecraft Services -> Profile.
//! Authorization Code + PKCE (S256) against a loopback redirect.
//!
//! Design rules enforced here:
//!   * The user's password NEVER touches this process. We open the SYSTEM browser,
//!     not an embedded WebView. Embedded auth UIs are a credential-phishing pattern
//!     and Microsoft explicitly discourages them.
//!   * No client secret. Desktop apps are public clients; a secret in a shipped
//!     binary is not a secret.
//!   * Refresh tokens are sealed with DPAPI (see `vault.rs`). Access tokens are
//!     held in `Zeroizing` memory and wiped on drop.
//!   * There is no offline/cracked code path compiled into this module. At all.

pub mod demo;
pub mod vault;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Response, Server};
use zeroize::Zeroizing;

/// Azure AD app registration (public client).
/// Replace with your own ID from portal.azure.com -> App registrations.
/// NOTE: this ID must additionally be approved for Minecraft API access,
/// otherwise api.minecraftservices.com returns 403 on leg 4.
const CLIENT_ID: &str = env!("ARSEX_AZURE_CLIENT_ID");

const AUTHORIZE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const MC_ENTITLEMENT_URL: &str =
    "https://api.minecraftservices.com/entitlements/mcstore";

const SCOPE: &str = "XboxLive.signin offline_access";

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub username: String,
    /// DPAPI-sealed refresh token. Opaque ciphertext; useless off this machine/user.
    pub sealed_refresh: Vec<u8>,
    pub skin_url: Option<String>,
    pub added_at: u64,
}

/// Live session. Deliberately NOT Serialize — this must never hit disk.
pub struct Session {
    pub uuid: String,
    pub username: String,
    pub access_token: Zeroizing<String>,
    pub expires_at: u64,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        now() + 60 >= self.expires_at // 60s safety margin
    }
}

#[derive(Deserialize)]
struct MsaTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<Xui>,
}

#[derive(Deserialize)]
struct Xui {
    uhs: String,
}

#[derive(Deserialize)]
struct XstsError {
    #[serde(rename = "XErr")]
    xerr: u64,
}

#[derive(Deserialize)]
struct McTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<McSkin>,
}

#[derive(Deserialize)]
struct McSkin {
    url: String,
    state: String,
}

#[derive(Deserialize)]
struct Entitlements {
    #[serde(default)]
    items: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------- PKCE

struct Pkce {
    verifier: Zeroizing<String>,
    challenge: String,
}

fn pkce() -> Pkce {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let verifier = URL_SAFE_NO_PAD.encode(raw);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Pkce { verifier: Zeroizing::new(verifier), challenge }
}

fn csrf_state() -> String {
    let mut raw = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut raw);
    URL_SAFE_NO_PAD.encode(raw)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

// ---------------------------------------------------------------- leg 1+2

/// Interactive login. Opens the system browser, listens on an ephemeral
/// loopback port, and completes the full chain.
pub async fn login_interactive() -> Result<(Account, Session)> {
    let pk = pkce();
    let state = csrf_state();

    // Bind :0 so the OS picks a free port. Never hardcode — collides with
    // other launchers and with a second Arsex instance.
    let server = Server::http("127.0.0.1:0")
        .map_err(|e| anyhow!("loopback bind failed: {e}"))?;
    let port = server.server_addr().to_ip().unwrap().port();
    let redirect = format!("http://127.0.0.1:{port}");

    let url = format!(
        "{AUTHORIZE_URL}?client_id={CLIENT_ID}&response_type=code\
         &redirect_uri={}&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256&prompt=select_account",
        urlencoding::encode(&redirect),
        urlencoding::encode(SCOPE),
        state,
        pk.challenge
    );

    open::that(&url).context("could not open system browser")?;

    // Block for the callback, with a hard timeout so a user who closes the
    // browser tab doesn't leak a listener thread forever.
    let code = tokio::task::spawn_blocking(move || -> Result<String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| anyhow!("login timed out after 5 minutes"))?;

            match server.recv_timeout(remaining)? {
                Some(req) => {
                    let q = req.url().to_string();
                    let params = parse_query(&q);

                    if let Some(err) = params.get("error") {
                        let _ = req.respond(Response::from_string(page(
                            "Sign-in cancelled",
                            "You can close this tab.",
                        )).with_header(html_header()));
                        bail!("authorization denied: {err}");
                    }

                    let got_state = params.get("state")
                        .ok_or_else(|| anyhow!("callback missing state"))?;
                    // Constant-time compare — this is the CSRF gate.
                    if !constant_eq(got_state.as_bytes(), state.as_bytes()) {
                        let _ = req.respond(Response::from_string("state mismatch"));
                        bail!("CSRF state mismatch — rejecting callback");
                    }

                    let code = params.get("code")
                        .ok_or_else(|| anyhow!("callback missing code"))?
                        .clone();

                    let _ = req.respond(Response::from_string(page(
                        "\u{65ac}",
                        "Signed in to Arsex. You can close this tab.",
                    )).with_header(html_header()));

                    return Ok(code);
                }
                None => bail!("login timed out after 5 minutes"),
            }
        }
    })
    .await??;

    // Leg 2: authorization code -> MSA tokens
    let http = client();
    let msa: MsaTokenResponse = http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &redirect),
            ("code_verifier", pk.verifier.as_str()),
        ])
        .send()
        .await?
        .error_for_status()
        .context("MSA token exchange failed")?
        .json()
        .await?;

    finish_chain(&http, msa).await
}

/// Silent re-auth from a stored refresh token. This is the path used on every
/// launch and on every account switch — target is < 400ms.
pub async fn login_silent(account: &Account) -> Result<(Account, Session)> {
    let refresh = vault::unseal(&account.sealed_refresh)
        .context("token vault unseal failed — machine or user changed?")?;

    let http = client();
    let msa: MsaTokenResponse = http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
            ("scope", SCOPE),
        ])
        .send()
        .await?
        .error_for_status()
        .context("refresh token rejected — interactive login required")?
        .json()
        .await?;

    finish_chain(&http, msa).await
}

// ---------------------------------------------------------------- legs 3-5

async fn finish_chain(
    http: &reqwest::Client,
    msa: MsaTokenResponse,
) -> Result<(Account, Session)> {
    // Leg 3: Xbox Live
    let xbl: XblResponse = http
        .post(XBL_URL)
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", msa.access_token)
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await?
        .error_for_status()
        .context("Xbox Live authentication failed")?
        .json()
        .await?;

    let uhs = xbl
        .display_claims
        .xui
        .first()
        .ok_or_else(|| anyhow!("XBL returned no user hash"))?
        .uhs
        .clone();

    // Leg 4: XSTS. This is where most launchers give users a useless error.
    let xsts_res = http
        .post(XSTS_URL)
        .json(&serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl.token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await?;

    if !xsts_res.status().is_success() {
        let body = xsts_res.text().await.unwrap_or_default();
        if let Ok(e) = serde_json::from_str::<XstsError>(&body) {
            // Translate Microsoft's numeric codes into something a human can act on.
            bail!(match e.xerr {
                2148916233 => "This Microsoft account has no Xbox profile. \
                               Sign in at xbox.com once to create one, then retry.",
                2148916235 => "Xbox Live is not available in this account's region.",
                2148916236 | 2148916237 =>
                    "This account requires adult verification (South Korea).",
                2148916238 => "This is a child account. An adult must add it to a \
                               Microsoft Family group before it can sign in.",
                _ => "Xbox Live rejected this account (XSTS).",
            });
        }
        bail!("XSTS authorization failed");
    }

    let xsts: XblResponse = xsts_res.json().await?;

    // Leg 5: Minecraft Services
    let mc: McTokenResponse = http
        .post(MC_LOGIN_URL)
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token)
        }))
        .send()
        .await?
        .error_for_status()
        .context("Minecraft Services login failed \
                  (is your Azure client ID approved for the Minecraft API?)")?
        .json()
        .await?;

    let access = Zeroizing::new(mc.access_token);

    // Ownership gate. A Game Pass or purchased account has entitlements;
    // an account that has never owned Java Edition has none.
    let ent: Entitlements = http
        .get(MC_ENTITLEMENT_URL)
        .bearer_auth(access.as_str())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    if ent.items.is_empty() {
        bail!("This account does not own Minecraft: Java Edition.");
    }

    let profile: McProfile = http
        .get(MC_PROFILE_URL)
        .bearer_auth(access.as_str())
        .send()
        .await?
        .error_for_status()
        .context("could not read Minecraft profile")?
        .json()
        .await?;

    let account = Account {
        uuid: profile.id.clone(),
        username: profile.name.clone(),
        sealed_refresh: vault::seal(&msa.refresh_token)?,
        skin_url: profile
            .skins
            .iter()
            .find(|s| s.state == "ACTIVE")
            .map(|s| s.url.clone()),
        added_at: now(),
    };

    let session = Session {
        uuid: profile.id,
        username: profile.name,
        access_token: access,
        expires_at: now() + mc.expires_in.min(msa.expires_in),
    };

    Ok((account, session))
}

// ---------------------------------------------------------------- helpers

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ArsexClient/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .https_only(true)
        .build()
        .expect("http client")
}

fn parse_query(url: &str) -> std::collections::HashMap<String, String> {
    url.split_once('?')
        .map(|(_, q)| {
            q.split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| {
                    (
                        k.to_string(),
                        urlencoding::decode(v).unwrap_or_default().into_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Non-short-circuiting compare. Overkill for a CSRF nonce, but free.
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn html_header() -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
        .unwrap()
}

/// The browser tab the user lands on. On-brand, monochrome, self-closing.
fn page(mark: &str, msg: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Arsex</title>
<style>
  html,body{{height:100%;margin:0;background:#000;color:#f5f5f5;
    font-family:"Segoe UI",system-ui,sans-serif;display:grid;place-items:center}}
  .m{{font-size:72px;opacity:0;filter:blur(16px);
     animation:i 1.2s cubic-bezier(.16,1,.3,1) forwards}}
  @keyframes i{{to{{opacity:1;filter:blur(0)}}}}
  p{{letter-spacing:.28em;font-size:11px;color:#8c8c8c;margin-top:26px}}
  .b{{width:200px;height:1px;background:#262626;margin-top:30px;overflow:hidden}}
  .b i{{display:block;height:100%;width:0;background:#fff;
       animation:d 1.6s cubic-bezier(.65,0,.35,1) .3s forwards}}
  @keyframes d{{to{{width:100%}}}}
</style></head><body><div style="text-align:center">
<div class="m">{mark}</div><p>{msg}</p><div class="b"><i></i></div></div>
<script>setTimeout(()=>window.close(),2200)</script></body></html>"#
    )
}

// ---------------------------------------------------------------- commands
//
// The Tauri boundary. These are the only auth entry points the webview can
// reach; note that no function here ever returns a token to the frontend —
// the renderer receives a display profile and nothing more. A token exposed
// to the webview is a token exposed to any XSS in the UI.

/// Persisted account list. Only sealed refresh tokens touch disk.
fn accounts_file() -> Result<std::path::PathBuf> {
    Ok(crate::paths::data_dir()?.join("accounts.json"))
}

fn load_accounts() -> Vec<Account> {
    accounts_file()
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_accounts(list: &[Account]) -> Result<()> {
    let p = accounts_file()?;
    std::fs::write(p, serde_json::to_vec_pretty(list)?)?;
    Ok(())
}

/// What the UI is allowed to know about a signed-in user.
#[derive(Serialize, Clone)]
pub struct PublicAccount {
    pub uuid: String,
    pub username: String,
    pub skin_url: Option<String>,
}

impl From<&Account> for PublicAccount {
    fn from(a: &Account) -> Self {
        Self {
            uuid: a.uuid.clone(),
            username: a.username.clone(),
            skin_url: a.skin_url.clone(),
        }
    }
}

#[tauri::command]
pub async fn begin_login() -> std::result::Result<PublicAccount, String> {
    let (account, _session) = login_interactive().await.map_err(|e| e.to_string())?;
    let mut list = load_accounts();
    list.retain(|a| a.uuid != account.uuid); // re-login replaces, never duplicates
    list.push(account.clone());
    save_accounts(&list).map_err(|e| e.to_string())?;
    Ok((&account).into())
}

#[tauri::command]
pub fn current_account() -> Option<PublicAccount> {
    load_accounts().last().map(PublicAccount::from)
}

#[tauri::command]
pub fn logout(uuid: String) -> std::result::Result<(), String> {
    let mut list = load_accounts();
    list.retain(|a| a.uuid != uuid);
    save_accounts(&list).map_err(|e| e.to_string())?;
    if list.is_empty() {
        // Last account gone: destroy the machine-bound entropy too, so the
        // sealed blobs left on disk are unrecoverable rather than merely unused.
        vault::purge().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Begin a demo session. Returns a synthetic profile that unlocks the UI for
/// testing but is structurally incapable of launching or joining a server.
#[tauri::command]
pub fn begin_demo(nickname: String) -> std::result::Result<demo::DemoProfile, String> {
    demo::start(&nickname)
}
