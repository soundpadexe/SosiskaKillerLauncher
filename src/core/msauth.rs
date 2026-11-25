//! Licensed (Microsoft) accounts: device-code sign-in → Xbox Live → XSTS →
//! Minecraft Services, plus skin changes on the real Mojang profile.
//!
//! Uses the client id of the official Minecraft launcher on login.live.com, which
//! Minecraft Services accepts without a separate Azure app approval.
use std::path::Path;

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::json;

const CLIENT_ID: &str = "00000000402b5328";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const DEVICE_URL: &str = "https://login.live.com/oauth20_connect.srf";
const TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const MC: &str = "https://api.minecraftservices.com";

/// Saved sign-in (`<data>/account.json`). Tokens never go into settings.json.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    /// Profile UUID without dashes.
    pub uuid: String,
    pub xuid: String,
    pub msa_refresh: String,
    pub mc_token: String,
    /// Unix seconds.
    pub mc_expires: u64,
}

impl Account {
    pub fn dashed_uuid(&self) -> String {
        let u = &self.uuid;
        if u.len() != 32 {
            return u.clone();
        }
        format!("{}-{}-{}-{}-{}", &u[0..8], &u[8..12], &u[12..16], &u[16..20], &u[20..32])
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok().filter(|a: &Account| !a.msa_refresh.is_empty())
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    #[serde(default = "five")]
    pub interval: u64,
    #[serde(default = "fifteen_min")]
    pub expires_in: u64,
}
fn five() -> u64 {
    5
}
fn fifteen_min() -> u64 {
    900
}

pub async fn device_code(client: &reqwest::Client) -> anyhow::Result<DeviceCode> {
    let r = client.post(DEVICE_URL).form(&[("client_id", CLIENT_ID), ("scope", SCOPE), ("response_type", "device_code")]).send().await?;
    Ok(r.error_for_status()?.json().await.context("Microsoft device code")?)
}

#[derive(Deserialize)]
struct MsaToken {
    access_token: String,
    refresh_token: String,
}

/// Waits until the user confirms the code in the browser. `cancelled` is polled between tries.
pub async fn wait_for_user(client: &reqwest::Client, dc: &DeviceCode, cancelled: impl Fn() -> bool) -> anyhow::Result<(String, String)> {
    let deadline = now() + dc.expires_in;
    let mut interval = dc.interval.max(1);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if cancelled() {
            bail!("cancelled");
        }
        if now() > deadline {
            bail!("the code expired, try again");
        }
        let r = client
            .post(TOKEN_URL)
            .form(&[("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("client_id", CLIENT_ID), ("device_code", dc.device_code.as_str())])
            .send()
            .await?;
        let v: serde_json::Value = r.json().await?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            match err {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval += 5;
                    continue;
                }
                "authorization_declined" | "access_denied" => bail!("sign-in was declined"),
                "expired_token" => bail!("the code expired, try again"),
                other => bail!("Microsoft: {}", v.get("error_description").and_then(|d| d.as_str()).unwrap_or(other)),
            }
        }
        let t: MsaToken = serde_json::from_value(v).context("Microsoft token")?;
        return Ok((t.access_token, t.refresh_token));
    }
}

async fn refresh_msa(client: &reqwest::Client, refresh: &str) -> anyhow::Result<(String, String)> {
    let r = client
        .post(TOKEN_URL)
        .form(&[("grant_type", "refresh_token"), ("client_id", CLIENT_ID), ("refresh_token", refresh), ("scope", SCOPE)])
        .send()
        .await?;
    if !r.status().is_success() {
        bail!("Microsoft session expired, sign in again");
    }
    let t: MsaToken = r.json().await?;
    Ok((t.access_token, t.refresh_token))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XblResp {
    token: String,
    display_claims: XblClaims,
}
#[derive(Deserialize)]
struct XblClaims {
    xui: Vec<serde_json::Value>,
}

/// MSA access token → Minecraft access token (+ xuid, expiry).
async fn minecraft_token(client: &reqwest::Client, msa: &str) -> anyhow::Result<(String, String, u64)> {
    let xbl: XblResp = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": msa },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await?
        .error_for_status()
        .context("Xbox Live")?
        .json()
        .await?;
    let r = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await?;
    if r.status() == reqwest::StatusCode::UNAUTHORIZED {
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        let msg = match v.get("XErr").and_then(|x| x.as_u64()) {
            Some(2148916233) => "this Microsoft account has no Xbox profile — open minecraft.net once and sign in there",
            Some(2148916235) => "Xbox Live is not available in your country",
            Some(2148916236) | Some(2148916237) => "this account needs adult verification on xbox.com",
            Some(2148916238) => "child account: an adult must add it to a Microsoft family",
            _ => "Xbox Live refused the sign-in",
        };
        bail!(msg);
    }
    let xsts: XblResp = r.error_for_status().context("XSTS")?.json().await?;
    let claim = xsts.display_claims.xui.first().ok_or_else(|| anyhow!("XSTS: no user claims"))?;
    let uhs = claim.get("uhs").and_then(|u| u.as_str()).unwrap_or_default();
    let xuid = claim.get("xid").and_then(|u| u.as_str()).unwrap_or("0").to_owned();

    #[derive(Deserialize)]
    struct McLogin {
        access_token: String,
        expires_in: u64,
    }
    let mc: McLogin = client
        .post(format!("{MC}/authentication/login_with_xbox"))
        .json(&json!({ "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token) }))
        .send()
        .await?
        .error_for_status()
        .context("Minecraft Services login")?
        .json()
        .await?;
    Ok((mc.access_token, xuid, now() + mc.expires_in))
}

#[derive(Clone, Debug, Deserialize)]
pub struct McSkin {
    pub state: String,
    pub url: String,
    #[serde(default)]
    pub variant: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct McProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<McSkin>,
}

impl McProfile {
    pub fn active_skin(&self) -> Option<&McSkin> {
        self.skins.iter().find(|s| s.state == "ACTIVE")
    }
}

pub async fn profile(client: &reqwest::Client, token: &str) -> anyhow::Result<McProfile> {
    let r = client.get(format!("{MC}/minecraft/profile")).bearer_auth(token).send().await?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("this account does not own Minecraft: Java Edition");
    }
    Ok(r.error_for_status().context("Minecraft profile")?.json().await?)
}

/// Full sign-in after the browser step. Returns the account and its current profile.
pub async fn finish_login(client: &reqwest::Client, msa_access: &str, msa_refresh: String) -> anyhow::Result<(Account, McProfile)> {
    let (mc_token, xuid, mc_expires) = minecraft_token(client, msa_access).await?;
    let p = profile(client, &mc_token).await?;
    Ok((Account { name: p.name.clone(), uuid: p.id.clone(), xuid, msa_refresh, mc_token, mc_expires }, p))
}

/// Makes sure the Minecraft token is valid for a while; refreshes the whole chain if not.
/// Returns true when the account changed and should be saved.
pub async fn ensure_fresh(client: &reqwest::Client, acc: &mut Account) -> anyhow::Result<bool> {
    if acc.mc_expires > now() + 600 && !acc.mc_token.is_empty() {
        return Ok(false);
    }
    let (msa, refresh) = refresh_msa(client, &acc.msa_refresh).await?;
    let (mc_token, xuid, mc_expires) = minecraft_token(client, &msa).await?;
    let p = profile(client, &mc_token).await?;
    *acc = Account { name: p.name, uuid: p.id, xuid, msa_refresh: refresh, mc_token, mc_expires };
    Ok(true)
}

/// Loads the saved account, refreshes it if needed and saves it back.
pub async fn fresh_from_disk(client: &reqwest::Client, path: &Path) -> anyhow::Result<Account> {
    let mut acc = Account::load(path).ok_or_else(|| anyhow!("not signed in to Microsoft"))?;
    if ensure_fresh(client, &mut acc).await? {
        acc.save(path)?;
    }
    Ok(acc)
}

/// Uploads a PNG skin to the Mojang profile (`None` resets to the default skin).
pub async fn set_skin(client: &reqwest::Client, token: &str, png: Option<Vec<u8>>, slim: bool) -> anyhow::Result<()> {
    let r = match png {
        Some(bytes) => {
            let part = reqwest::multipart::Part::bytes(bytes).file_name("skin.png").mime_str("image/png")?;
            let form = reqwest::multipart::Form::new().text("variant", if slim { "slim" } else { "classic" }).part("file", part);
            client.post(format!("{MC}/minecraft/profile/skins")).bearer_auth(token).multipart(form).send().await?
        }
        None => client.delete(format!("{MC}/minecraft/profile/skins/active")).bearer_auth(token).send().await?,
    };
    if !r.status().is_success() {
        let code = r.status();
        let body = r.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("errorMessage").or_else(|| v.get("error")).and_then(|m| m.as_str()).map(str::to_owned))
            .unwrap_or(body);
        bail!("Mojang rejected the skin ({code}): {msg}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dashes() {
        let a = super::Account { uuid: "069a79f444e94726a5befca90e38aaf5".into(), ..Default::default() };
        assert_eq!(a.dashed_uuid(), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    }
}
