//! Mobile & embedded account manager: supports **Offline Mode** (default, zero
//! login gate, customizable player name for quick testing/offline play) alongside
//! full **Microsoft OAuth 2.0 Device-Code -> Xbox Live -> XSTS -> Minecraft
//! Services** genuine authentication, automatic token renewal, and pluggable
//! hardware-keystore secret sealing.

use super::{AuthSession, parse_uuid, prepare_chat_keys, set_current_opt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_OFFLINE_NAME: &str = "Player";
pub const STORE_FILENAME: &str = "android_accounts.json";
const SCOPE: &str = "XboxLive.signin offline_access";
const RENEW_BEFORE_SECS: u64 = 3600;

/// Active play identity mode: defaults to `Offline` so launching the game
/// never forces a sign-in gate during development or offline play.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountMode {
    #[default]
    Offline,
    Microsoft {
        uuid: String,
    },
}

/// Public (non-secret) metadata of a signed-in Minecraft: Java Edition profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccount {
    /// 32-character lowercase hex UUID without dashes.
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub skin_url: Option<String>,
    #[serde(default)]
    pub skin_variant: Option<String>,
    #[serde(default)]
    pub cape_url: Option<String>,
}

/// Sensitive tokens sealed via Android Hardware Keystore (never stored in
/// `android_accounts.json` or written to logs).
#[derive(Clone, Serialize, Deserialize)]
pub struct AccountSecrets {
    pub refresh_token: String,
    pub minecraft_token: String,
    pub minecraft_expires_at: u64,
    #[serde(default)]
    pub xuid: Option<String>,
    #[serde(default = "default_true")]
    pub owns_game: bool,
    #[serde(default)]
    pub sign_in_required: bool,
}

fn default_true() -> bool {
    true
}

impl std::fmt::Debug for AccountSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountSecrets(<sealed>)")
    }
}

/// Non-sensitive account selection & profile list stored in `<game_dir>/android_accounts.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountStore {
    #[serde(default)]
    pub mode: AccountMode,
    #[serde(default = "default_offline_name")]
    pub offline_name: String,
    #[serde(default)]
    pub custom_client_id: String,
    #[serde(default)]
    pub accounts: Vec<StoredAccount>,
}

fn default_offline_name() -> String {
    DEFAULT_OFFLINE_NAME.to_owned()
}

impl Default for AccountStore {
    fn default() -> Self {
        Self {
            mode: AccountMode::Offline,
            offline_name: DEFAULT_OFFLINE_NAME.to_owned(),
            custom_client_id: String::new(),
            accounts: Vec::new(),
        }
    }
}

/// Validates and trims an offline player name (`1..=16` ASCII `[A-Za-z0-9_]`).
pub fn sanitize_offline_name(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 16 {
        return None;
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

impl AccountStore {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(STORE_FILENAME);
        let mut store: Self = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if sanitize_offline_name(&store.offline_name).is_none() {
            store.offline_name = DEFAULT_OFFLINE_NAME.to_owned();
        }
        store.custom_client_id = store.custom_client_id.trim().to_owned();
        store
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(STORE_FILENAME);
        let tmp = dir.join("android_accounts.json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    /// Resolves the Azure Application (client) ID: user custom override first,
    /// then runtime env `HORIZON_MSA_CLIENT_ID`, then compile-time `HORIZON_MSA_CLIENT_ID`.
    pub fn effective_client_id(&self) -> Option<String> {
        if !self.custom_client_id.trim().is_empty() {
            return Some(self.custom_client_id.trim().to_owned());
        }
        builtin_client_id()
    }

    pub fn upsert_account(&mut self, account: StoredAccount) {
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
    }

    pub fn remove_account(&mut self, uuid: &str) {
        let norm = uuid.replace('-', "").to_lowercase();
        self.accounts.retain(|a| a.uuid != norm);
        if matches!(&self.mode, AccountMode::Microsoft { uuid: u } if u == &norm) {
            self.mode = AccountMode::Offline;
        }
    }
}

pub fn builtin_client_id() -> Option<String> {
    std::env::var("HORIZON_MSA_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("HORIZON_MSA_CLIENT_ID").map(str::to_owned))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

// ----- Pluggable Hardware Secret Vault -----------------------------------------------------------

struct SecretVaultCallbacks {
    get: fn(&str) -> Option<String>,
    set: fn(&str, &str),
    delete: fn(&str),
}

static SECRET_VAULT: OnceLock<SecretVaultCallbacks> = OnceLock::new();
static FALLBACK_SECRETS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn set_secret_store_callbacks(
    get: fn(&str) -> Option<String>,
    set: fn(&str, &str),
    delete: fn(&str),
) {
    let _ = SECRET_VAULT.set(SecretVaultCallbacks { get, set, delete });
}

fn secret_key(uuid: &str) -> String {
    format!("msa_account_{}", uuid.replace('-', "").to_lowercase())
}

pub fn load_secrets(uuid: &str) -> Option<AccountSecrets> {
    let key = secret_key(uuid);
    let raw = if let Some(vault) = SECRET_VAULT.get() {
        (vault.get)(&key)?
    } else {
        FALLBACK_SECRETS.lock().ok()?.get(&key).cloned()?
    };
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str(&raw).ok()
}

pub fn save_secrets(uuid: &str, secrets: &AccountSecrets) {
    let Ok(raw) = serde_json::to_string(secrets) else {
        return;
    };
    let key = secret_key(uuid);
    if let Some(vault) = SECRET_VAULT.get() {
        (vault.set)(&key, &raw);
    } else if let Ok(mut map) = FALLBACK_SECRETS.lock() {
        map.insert(key, raw);
    }
}

pub fn delete_secrets(uuid: &str) {
    let key = secret_key(uuid);
    if let Some(vault) = SECRET_VAULT.get() {
        (vault.delete)(&key);
    } else if let Ok(mut map) = FALLBACK_SECRETS.lock() {
        map.remove(&key);
    }
}

// ----- HTTP Endpoints & Happy-Eyeballs Resolver --------------------------------------------------

#[derive(Clone, Debug)]
pub struct Endpoints {
    pub microsoft: String,
    pub xbox_user: String,
    pub xsts: String,
    pub minecraft: String,
}

impl Endpoints {
    pub fn live() -> Self {
        Self {
            microsoft: "https://login.microsoftonline.com/consumers/oauth2/v2.0".into(),
            xbox_user: "https://user.auth.xboxlive.com/user/authenticate".into(),
            xsts: "https://xsts.auth.xboxlive.com/xsts/authorize".into(),
            minecraft: "https://api.minecraftservices.com".into(),
        }
    }
}

fn interleave(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(first) = addrs.first().map(SocketAddr::is_ipv6) else {
        return addrs;
    };
    let (mut same, mut other): (Vec<_>, Vec<_>) =
        addrs.into_iter().partition(|a| a.is_ipv6() == first);
    same.reverse();
    other.reverse();
    let mut out = Vec::with_capacity(same.len() + other.len());
    while !same.is_empty() || !other.is_empty() {
        out.extend(same.pop());
        out.extend(other.pop());
    }
    out
}

struct InterleavedResolver;

impl ureq::Resolver for InterleavedResolver {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        netloc.to_socket_addrs().map(|it| interleave(it.collect()))
    }
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .timeout_connect(Duration::from_secs(10))
        .resolver(InterleavedResolver)
        .user_agent(&format!("Horizon-Android/{}", horizon_core::release::VERSION))
        .build()
}

enum ReqBody<'a> {
    Form(Vec<(&'a str, &'a str)>),
    Json(Value),
}

fn send_http(
    method: &str,
    url: &str,
    bearer_token: Option<&str>,
    body: Option<ReqBody<'_>>,
    service_label: &str,
) -> Result<(u16, Value), String> {
    let agent = http_agent();
    let mut req = agent.request(method, url);
    if let Some(tok) = bearer_token {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }
    let result = match body {
        None => req.call(),
        Some(ReqBody::Form(fields)) => req.send_form(&fields),
        Some(ReqBody::Json(val)) => req
            .set("Accept", "application/json")
            .set("Content-Type", "application/json; charset=utf-8")
            .send_string(&val.to_string()),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(format!("无法连接 {service_label}，请检查手机网络连接。"));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ----- Step 1 & 2: OAuth 2.0 Device Code & Token Polling -----------------------------------------

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_at: u64,
    device_code: String,
}

impl std::fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .finish_non_exhaustive()
    }
}

pub struct MsTokens {
    pub access: String,
    pub refresh: String,
}

impl std::fmt::Debug for MsTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MsTokens(<hidden>)")
    }
}

pub enum DevicePoll {
    Pending,
    SlowDown,
    Done(MsTokens),
}

fn oauth_error_message(v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => "浏览器中拒绝了登录授权。".into(),
        Some("expired_token") => "设备验证码已过期，请重新发起登录。".into(),
        Some("bad_verification_code") => "验证码无效，请重新获取。".into(),
        Some("invalid_grant") => "微软登录凭证已过期，请重新登录。".into(),
        Some("invalid_client" | "unauthorized_client") => {
            "微软拒绝了当前 Client ID（请确认 Azure 应用已开启 Allow public client flows）。".into()
        }
        Some(other) => format!("微软登录失败 ({other})。"),
        None => "微软登录请求失败。".into(),
    }
}

pub fn request_device_code(ep: &Endpoints, client_id: &str) -> Result<DeviceCode, String> {
    let cid = client_id.trim();
    if cid.is_empty() {
        return Err(
            "尚未配置微软 Client ID：请在下方「高级 / Client ID 配置」中填入你的 Azure 应用 Client ID。"
                .into(),
        );
    }
    let (status, v) = send_http(
        "POST",
        &format!("{}/devicecode", ep.microsoft),
        None,
        Some(ReqBody::Form(vec![("client_id", cid), ("scope", SCOPE)])),
        "Microsoft 登录服务",
    )?;
    match (
        status,
        str_field(&v, "user_code"),
        str_field(&v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            verification_uri: str_field(&v, "verification_uri")
                .unwrap_or_else(|| "https://www.microsoft.com/link".into()),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .clamp(1, 60),
            expires_at: now_secs().saturating_add(
                v.get("expires_in")
                    .and_then(Value::as_u64)
                    .unwrap_or(900)
                    .min(3600),
            ),
            device_code,
        }),
        _ => Err(oauth_error_message(&v)),
    }
}

pub fn poll_device_token(
    ep: &Endpoints,
    client_id: &str,
    code: &DeviceCode,
) -> Result<DevicePoll, String> {
    let (status, v) = send_http(
        "POST",
        &format!("{}/token", ep.microsoft),
        None,
        Some(ReqBody::Form(vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.trim()),
            ("device_code", &code.device_code),
        ])),
        "Microsoft 登录服务",
    )?;
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => return Ok(DevicePoll::Pending),
        Some("slow_down") => return Ok(DevicePoll::SlowDown),
        _ => {}
    }
    parse_ms_tokens(status, &v).map(DevicePoll::Done)
}

pub fn wait_for_device_tokens(
    ep: &Endpoints,
    client_id: &str,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
) -> Result<MsTokens, String> {
    let mut interval = code.interval;
    loop {
        let deadline = std::time::Instant::now() + Duration::from_secs(interval);
        while std::time::Instant::now() < deadline {
            if cancelled() {
                return Err("已取消登录。".into());
            }
            std::thread::sleep(Duration::from_millis(120));
        }
        if now_secs() > code.expires_at {
            return Err("验证码已过期，请重新点击登录。".into());
        }
        match poll_device_token(ep, client_id, code)? {
            DevicePoll::Pending => {}
            DevicePoll::SlowDown => interval = (interval + 5).min(60),
            DevicePoll::Done(tokens) => return Ok(tokens),
        }
    }
}

fn parse_ms_tokens(status: u16, v: &Value) -> Result<MsTokens, String> {
    match (
        status,
        str_field(v, "access_token"),
        str_field(v, "refresh_token"),
    ) {
        (200, Some(access), Some(refresh)) => Ok(MsTokens { access, refresh }),
        _ => Err(oauth_error_message(v)),
    }
}

fn refresh_ms_token(ep: &Endpoints, client_id: &str, refresh: &str) -> Result<MsTokens, String> {
    let (status, v) = send_http(
        "POST",
        &format!("{}/token", ep.microsoft),
        None,
        Some(ReqBody::Form(vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id.trim()),
            ("refresh_token", refresh),
            ("scope", SCOPE),
        ])),
        "Microsoft 登录服务",
    )?;
    parse_ms_tokens(status, &v)
}

// ----- Step 3..7: Xbox Live -> XSTS -> Minecraft Token -> Entitlements -> Profile ----------------

fn xsts_error_message(code: u64) -> String {
    match code {
        2148916233 => {
            "该微软账号尚未创建 Xbox 档案，请先在浏览器打开 xbox.com 登录一次创建后再试。".into()
        }
        2148916235 => "当前国家/地区不支持 Xbox Live 服务。".into(),
        2148916236 | 2148916237 => "该账号需要先在 xbox.com 完成成人身份验证。".into(),
        2148916238 => "该账号为未成年人账号，需先由监护人加入微软家庭组并开启多人游戏权限。".into(),
        other => format!("Xbox Live 授权被拒绝 (错误码 {other})。"),
    }
}

struct MinecraftAuth {
    token: String,
    expires_at: u64,
    xuid: Option<String>,
}

fn login_minecraft_via_xbox(
    ep: &Endpoints,
    ms_access: &str,
    on_phase: &dyn Fn(&str),
) -> Result<MinecraftAuth, String> {
    on_phase("正在验证 Xbox Live 身份…");
    let (status, xbl) = send_http(
        "POST",
        &ep.xbox_user,
        None,
        Some(ReqBody::Json(json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    let xbl_token = str_field(&xbl, "Token")
        .filter(|_| status == 200)
        .ok_or_else(|| "Xbox Live 认证未通过。".to_owned())?;

    let (status, xsts) = send_http(
        "POST",
        &ep.xsts,
        None,
        Some(ReqBody::Json(json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))),
        "Xbox XSTS",
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error_message(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(xsts_token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err("Xbox XSTS 授权未返回有效凭证。".into());
    };
    let xuid = claims.and_then(|c| str_field(c, "xid"));

    on_phase("正在换取 Minecraft 正版会话令牌…");
    let xtoken = format!("XBL3.0 x={uhs};{xsts_token}");
    let (token, expires_in) = fetch_minecraft_token(ep, &xtoken)?;
    Ok(MinecraftAuth {
        token,
        expires_at: now_secs().saturating_add(expires_in.min(7 * 86_400)),
        xuid,
    })
}

fn fetch_minecraft_token(ep: &Endpoints, xtoken: &str) -> Result<(String, u64), String> {
    let lifetime = |v: &Value| v.get("expires_in").and_then(Value::as_u64).unwrap_or(86_400);
    if let Ok((200, v)) = send_http(
        "POST",
        &format!("{}/launcher/login", ep.minecraft),
        None,
        Some(ReqBody::Json(
            json!({ "xtoken": xtoken, "platform": "PC_LAUNCHER" }),
        )),
        "Minecraft Services",
    ) && let Some(tok) = str_field(&v, "access_token")
    {
        return Ok((tok, lifetime(&v)));
    }

    let (status, mc) = send_http(
        "POST",
        &format!("{}/authentication/login_with_xbox", ep.minecraft),
        None,
        Some(ReqBody::Json(json!({ "identityToken": xtoken }))),
        "Minecraft Services",
    )?;
    match (status, str_field(&mc, "access_token")) {
        (200, Some(tok)) => Ok((tok, lifetime(&mc))),
        (403, _) => Err(
            "Minecraft 服务拒绝了该 Client ID (HTTP 403)：新注册的 Azure 应用需通过 Mojang App ID 审核 (aka.ms/mce-reviewappid) 后才可换取令牌。"
                .into(),
        ),
        (429, _) => Err("登录请求过于频繁 (HTTP 429)，请稍候一分钟再试。".into()),
        _ => Err(format!("Minecraft 令牌换取失败 (HTTP {status})。")),
    }
}

fn check_owns_java_edition(ep: &Endpoints, mc_token: &str) -> Result<bool, String> {
    let (status, v) = send_http(
        "GET",
        &format!("{}/entitlements/mcstore", ep.minecraft),
        Some(mc_token),
        None,
        "Minecraft Services",
    )?;
    if status != 200 {
        return Err("无法查询账号的 Minecraft 正版购买资格。".into());
    }
    Ok(v.get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|i| {
                matches!(
                    i.get("name").and_then(Value::as_str),
                    Some("game_minecraft" | "product_minecraft")
                )
            })
        }))
}

fn fetch_minecraft_profile(ep: &Endpoints, mc_token: &str) -> Result<Option<StoredAccount>, String> {
    let (status, v) = send_http(
        "GET",
        &format!("{}/minecraft/profile", ep.minecraft),
        Some(mc_token),
        None,
        "Minecraft Services",
    )?;
    if status == 404 {
        return Ok(None);
    }
    let (Some(uuid), Some(name)) = (str_field(&v, "id"), str_field(&v, "name")) else {
        return Err("无法解析 Minecraft 玩家档案。".into());
    };
    let active_entry = |key: &str| {
        v.get(key)
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|s| s.get("state").and_then(Value::as_str) == Some("ACTIVE"))
            })
            .cloned()
    };
    let active_skin = active_entry("skins");
    Ok(Some(StoredAccount {
        uuid: uuid.replace('-', "").to_lowercase(),
        name,
        skin_url: active_skin.as_ref().and_then(|s| str_field(s, "url")),
        skin_variant: active_skin.as_ref().map(|s| {
            str_field(s, "variant")
                .unwrap_or_else(|| "CLASSIC".into())
                .to_lowercase()
        }),
        cape_url: active_entry("capes").and_then(|c| str_field(&c, "url")),
    }))
}

/// Completes sign-in once Microsoft device-code polling yields `MsTokens`.
pub fn finish_sign_in(
    ep: &Endpoints,
    ms: MsTokens,
    on_phase: &dyn Fn(&str),
) -> Result<(StoredAccount, AccountSecrets), String> {
    let mc = login_minecraft_via_xbox(ep, &ms.access, on_phase)?;
    on_phase("正在检查 Java 版正版资格与玩家档案…");
    let owns = check_owns_java_edition(ep, &mc.token)?;
    if !owns {
        return Err("该微软账号未拥有《Minecraft: Java 版》（含 XGP 会员资格）。".into());
    }
    let account = fetch_minecraft_profile(ep, &mc.token)?.ok_or_else(|| {
        "该账号尚未创建 Java 版玩家角色名，请先前往 minecraft.net 设置游戏 ID 后重试。".to_owned()
    })?;
    let secrets = AccountSecrets {
        refresh_token: ms.refresh,
        minecraft_token: mc.token,
        minecraft_expires_at: mc.expires_at,
        xuid: mc.xuid,
        owns_game: true,
        sign_in_required: false,
    };
    Ok((account, secrets))
}

pub fn session_from_account(account: &StoredAccount, secrets: &AccountSecrets) -> Option<AuthSession> {
    let uuid = parse_uuid(&account.uuid)?;
    if secrets.minecraft_token.is_empty() {
        return None;
    }
    Some(AuthSession::with_xuid(
        &account.name,
        uuid,
        secrets.xuid.clone(),
        &secrets.minecraft_token,
    ))
}

// ----- Session Resolution, Renewal & Offline Fallback --------------------------------------------

#[derive(Clone, Debug)]
pub struct ResolvedIdentity {
    pub player_name: String,
    pub session: Option<AuthSession>,
    pub is_microsoft: bool,
    pub notice: Option<String>,
}

/// Resolves and activates the player's identity from `dir/android_accounts.json`.
/// - In `AccountMode::Offline`, returns immediately (`0ms`, zero network calls) with
///   `session: None` and clears `auth::current()`.
/// - In `AccountMode::Microsoft`, renews the token if `< 1h` remains; if offline/unreachable,
///   gracefully keeps the cached Microsoft player name offline without blocking play.
pub fn resolve_and_activate(dir: &Path, ep: &Endpoints, force_refresh: bool) -> ResolvedIdentity {
    let mut store = AccountStore::load(dir);
    let identity = match &store.mode {
        AccountMode::Offline => ResolvedIdentity {
            player_name: store.offline_name.clone(),
            session: None,
            is_microsoft: false,
            notice: None,
        },
        AccountMode::Microsoft { uuid } => {
            let norm = uuid.replace('-', "").to_lowercase();
            if let Some(account) = store.accounts.iter().find(|a| a.uuid == norm).cloned() {
                if let Some(mut secrets) = load_secrets(&norm) {
                    let needs_renew = force_refresh
                        || secrets.minecraft_expires_at <= now_secs() + RENEW_BEFORE_SECS;
                    if needs_renew && !secrets.refresh_token.is_empty() {
                        if let Some(cid) = store.effective_client_id() {
                            match refresh_ms_token(ep, &cid, &secrets.refresh_token) {
                                Ok(ms) => {
                                    secrets.refresh_token = ms.refresh;
                                    match login_minecraft_via_xbox(ep, &ms.access, &|_| {}) {
                                        Ok(mc) => {
                                            secrets.minecraft_token = mc.token;
                                            secrets.minecraft_expires_at = mc.expires_at;
                                            secrets.xuid = mc.xuid;
                                            secrets.sign_in_required = false;
                                            if let Ok(Some(updated)) =
                                                fetch_minecraft_profile(ep, &secrets.minecraft_token)
                                                && updated.uuid == account.uuid
                                            {
                                                store.upsert_account(updated.clone());
                                                let _ = store.save(dir);
                                                save_secrets(&norm, &secrets);
                                                let sess = session_from_account(&updated, &secrets);
                                                return apply_resolved(ResolvedIdentity {
                                                    player_name: updated.name,
                                                    session: sess,
                                                    is_microsoft: true,
                                                    notice: None,
                                                });
                                            }
                                            save_secrets(&norm, &secrets);
                                        }
                                        Err(e) => {
                                            save_secrets(&norm, &secrets);
                                            let still_valid =
                                                secrets.minecraft_expires_at > now_secs() + 60;
                                            let sess = still_valid
                                                .then(|| session_from_account(&account, &secrets))
                                                .flatten();
                                            return apply_resolved(ResolvedIdentity {
                                                player_name: account.name,
                                                session: sess,
                                                is_microsoft: true,
                                                notice: Some(format!("令牌刷新未完成（已离线保活）：{e}")),
                                            });
                                        }
                                    }
                                }
                                Err(e) => {
                                    let still_valid = secrets.minecraft_expires_at > now_secs() + 60;
                                    let sess = still_valid
                                        .then(|| session_from_account(&account, &secrets))
                                        .flatten();
                                    return apply_resolved(ResolvedIdentity {
                                        player_name: account.name,
                                        session: sess,
                                        is_microsoft: true,
                                        notice: Some(format!("微软令牌续期失败（当前仅可单机/离线联机）：{e}")),
                                    });
                                }
                            }
                        }
                    }
                    let valid = secrets.minecraft_expires_at > now_secs() + 60;
                    let sess = valid
                        .then(|| session_from_account(&account, &secrets))
                        .flatten();
                    ResolvedIdentity {
                        player_name: account.name,
                        session: sess,
                        is_microsoft: true,
                        notice: if valid {
                            None
                        } else {
                            Some("正版会话已过期，重新连接网络或刷新令牌后可进正版服。".into())
                        },
                    }
                } else {
                    ResolvedIdentity {
                        player_name: account.name,
                        session: None,
                        is_microsoft: false,
                        notice: Some("未找到该正版账号的本地令牌，请重新登录或切换为离线模式。".into()),
                    }
                }
            } else {
                store.mode = AccountMode::Offline;
                let _ = store.save(dir);
                ResolvedIdentity {
                    player_name: store.offline_name,
                    session: None,
                    is_microsoft: false,
                    notice: None,
                }
            }
        }
    };
    apply_resolved(identity)
}

fn apply_resolved(identity: ResolvedIdentity) -> ResolvedIdentity {
    let has_session = identity.session.is_some();
    set_current_opt(identity.session.clone());
    if has_session {
        prepare_chat_keys(true);
    }
    identity
}

/// Imports a raw session JSON (`{"name","uuid","access_token","user_type":"msa","xuid"}`)
/// directly for developer testing without requiring a device-code round-trip.
pub fn import_session_json(dir: &Path, json_text: &str) -> Result<ResolvedIdentity, String> {
    let session = AuthSession::parse(json_text.trim())?;
    let uuid_hex = format!("{:032x}", session.uuid);
    let mut store = AccountStore::load(dir);
    let account = StoredAccount {
        uuid: uuid_hex.clone(),
        name: session.name.clone(),
        skin_url: None,
        skin_variant: None,
        cape_url: None,
    };
    let secrets = AccountSecrets {
        refresh_token: String::new(),
        minecraft_token: session.access_token().to_owned(),
        minecraft_expires_at: now_secs() + 24 * 3600,
        xuid: session.xuid.clone(),
        owns_game: true,
        sign_in_required: false,
    };
    save_secrets(&uuid_hex, &secrets);
    store.upsert_account(account);
    store.mode = AccountMode::Microsoft { uuid: uuid_hex };
    store.save(dir)?;
    Ok(apply_resolved(ResolvedIdentity {
        player_name: session.name.clone(),
        session: Some(session),
        is_microsoft: true,
        notice: None,
    }))
}

/// JSON summary consumed by `HorizonActivity`'s Account Center dialog.
pub fn ui_state_json(dir: &Path) -> String {
    let store = AccountStore::load(dir);
    let active_uuid = match &store.mode {
        AccountMode::Offline => "",
        AccountMode::Microsoft { uuid } => uuid.as_str(),
    };
    let online_ready = super::current().is_some_and(|s| !s.expired());
    let accounts: Vec<Value> = store
        .accounts
        .iter()
        .map(|a| {
            let active = a.uuid == active_uuid;
            json!({
                "uuid": a.uuid,
                "name": a.name,
                "active": active,
                "online_ready": active && online_ready,
            })
        })
        .collect();
    json!({
        "mode": if active_uuid.is_empty() { "offline" } else { "microsoft" },
        "active_uuid": active_uuid,
        "offline_name": store.offline_name,
        "custom_client_id": store.custom_client_id,
        "has_builtin_client_id": builtin_client_id().is_some(),
        "effective_client_id": store.effective_client_id().unwrap_or_default(),
        "accounts": accounts,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn spawn_mock_auth_server() -> (Endpoints, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let ep = Endpoints {
            microsoft: format!("{base}/ms"),
            xbox_user: format!("{base}/xbl"),
            xsts: format!("{base}/xsts"),
            minecraft: format!("{base}/mc"),
        };
        let handle = std::thread::spawn(move || {
            for _ in 0..7 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let body_json = if first_line.contains("/ms/devicecode") {
                    json!({
                        "user_code": "ABCD-1234",
                        "device_code": "dev-secret-code",
                        "verification_uri": "https://www.microsoft.com/link",
                        "interval": 1,
                        "expires_in": 900
                    })
                } else if first_line.contains("/ms/token") {
                    json!({
                        "access_token": "ms-access-tok",
                        "refresh_token": "ms-refresh-tok"
                    })
                } else if first_line.contains("/xbl") {
                    json!({ "Token": "xbl-tok" })
                } else if first_line.contains("/xsts") {
                    json!({
                        "Token": "xsts-tok",
                        "DisplayClaims": { "xui": [{ "uhs": "uhs123", "xid": "253541234567" }] }
                    })
                } else if first_line.contains("/mc/launcher/login") {
                    json!({ "access_token": "mc-jwt-access-token", "expires_in": 86400 })
                } else if first_line.contains("/mc/entitlements/mcstore") {
                    json!({ "items": [{ "name": "game_minecraft" }] })
                } else {
                    json!({
                        "id": "069a79f444e94726a5befca90e38aaf5",
                        "name": "NotchTest",
                        "skins": [{ "state": "ACTIVE", "url": "https://textures.minecraft.net/texture/abc", "variant": "SLIM" }],
                        "capes": []
                    })
                };
                let payload = body_json.to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (ep, handle)
    }

    #[test]
    fn default_store_starts_in_offline_mode_without_network() {
        let dir = std::env::temp_dir().join(format!("horizon-msa-offline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let resolved = resolve_and_activate(&dir, &Endpoints::live(), false);
        assert_eq!(resolved.player_name, "Player");
        assert!(resolved.session.is_none());
        assert!(!resolved.is_microsoft);
        assert!(super::super::current().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_name_validation_accepts_minecraft_identifiers_and_rejects_bad_ones() {
        assert_eq!(sanitize_offline_name("  Alex_26  ").as_deref(), Some("Alex_26"));
        assert_eq!(sanitize_offline_name(""), None);
        assert_eq!(sanitize_offline_name("TooLongPlayerName17"), None);
        assert_eq!(sanitize_offline_name("bad space"), None);
    }

    #[test]
    fn full_seven_step_device_code_to_minecraft_profile_and_offline_switch_works() {
        let dir = std::env::temp_dir().join(format!("horizon-msa-flow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (ep, handle) = spawn_mock_auth_server();

        let dc = request_device_code(&ep, "00000000-1111-2222-3333-444444444444").unwrap();
        assert_eq!(dc.user_code, "ABCD-1234");
        let DevicePoll::Done(ms) =
            poll_device_token(&ep, "00000000-1111-2222-3333-444444444444", &dc).unwrap()
        else {
            panic!("expected Done");
        };
        let (account, secrets) = finish_sign_in(&ep, ms, &|_| {}).unwrap();
        handle.join().unwrap();

        assert_eq!(account.name, "NotchTest");
        assert_eq!(account.uuid, "069a79f444e94726a5befca90e38aaf5");
        assert_eq!(account.skin_variant.as_deref(), Some("slim"));

        save_secrets(&account.uuid, &secrets);
        let mut store = AccountStore::load(&dir);
        store.offline_name = "DevTester".into();
        store.upsert_account(account.clone());
        store.mode = AccountMode::Microsoft {
            uuid: account.uuid.clone(),
        };
        store.save(&dir).unwrap();

        // Activating Microsoft mode sets auth::current() to NotchTest!
        let active_ms = resolve_and_activate(&dir, &Endpoints::live(), false);
        assert_eq!(active_ms.player_name, "NotchTest");
        assert!(active_ms.session.is_some());
        assert_eq!(super::super::current().unwrap().name, "NotchTest");

        // Switching back to Offline mode immediately clears auth::current() and uses DevTester!
        store.mode = AccountMode::Offline;
        store.save(&dir).unwrap();
        let active_off = resolve_and_activate(&dir, &Endpoints::live(), false);
        assert_eq!(active_off.player_name, "DevTester");
        assert!(active_off.session.is_none());
        assert!(super::super::current().is_none());

        delete_secrets(&account.uuid);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
