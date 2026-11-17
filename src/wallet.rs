//! Sign-in with Tempo ID (id.tempo.xyz) for machine payments.
//!
//! The library asks for an access key through Tempo ID's device grant (RFC 8628): it posts the key's
//! permissions to `/device/code` as `key_authorization`, the owner approves the code on Tempo ID's
//! device page with a passkey, and `/oauth2/token` returns the account and the owner's signed key
//! authorization, which mpp-rs attaches to the key's first transaction.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use mpp::{
    Address, PrivateKeySigner,
    client::TempoAccountsProvider,
    protocol::methods::tempo::{CHAIN_ID, USDC},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempo_alloy::{
    accounts::TempoAccountsWallet,
    primitives::transaction::key_authorization::SignedKeyAuthorization,
};

use crate::{bridge::Host, payments::Payments};

pub const ISSUER: &str = "https://id.tempo.xyz";
/// An unregistered CLI's client ID, which Tempo ID accepts without registration and lets request an
/// access key through the device grant.
pub const CLIENT_ID: &str = "cli:nanocode";
const SCOPE: &str = "openid";
const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const ALLOWANCE: u128 = 2_000_000;
const FEE_ALLOWANCE: u128 = 100_000;
/// The MACH balance below which Tempo ID opens its deposit page after the approval, and the amount
/// the funding link asks for: the key's daily allowance.
const FUNDING_HINT: &str = "2";
const FUNDING_TOKEN: &str = "MACH";
const PATH_USD: &str = "0x20c0000000000000000000000000000000000000";
const LIFETIME: Duration = Duration::from_secs(86_400);
const ESCROW: &str = "0x4d50500000000000000000000000000000000000";
const SWAPPER: &str = "0xF72E5107c32C655ffA7539a3C8e97B7C3cE16A3F";
const POLL_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);

const APPROVE: &str = "0x095ea7b3";
const OPEN: &str = "0xedc53b00";
const REQUEST_CLOSE: &str = "0x675402e5";
const SWAP_TO: &str = "0x34189fed";
const TOP_UP: &str = "0xdc48471e";
const TRANSFER_WITH_MEMO: &str = "0x95777d59";
const WITHDRAW: &str = "0x41e2c664";

#[derive(Clone, Deserialize, Serialize)]
pub struct Record {
    pub account: String,
    pub key: String,
    pub key_address: String,
    pub expiry: u64,
    /// The owner's signed key authorization, as Tempo ID returns it (the Tempo node's RPC format).
    pub authorization: Value,
}

#[derive(Deserialize)]
pub struct Meta {
    /// The device name that Tempo ID's device page shows, such as `nanocode for macOS`.
    #[serde(default)]
    pub name: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(2 + bytes.len() * 2);
    text.push_str("0x");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn quantity(value: u128) -> String {
    format!("0x{value:x}")
}

/// Reads Tempo ID's key authorization and checks that it approves `key` on Tempo mainnet.
pub fn authorization(value: &Value, key: Address) -> Result<SignedKeyAuthorization, String> {
    let signed: SignedKeyAuthorization = serde_json::from_value(value.clone())
        .map_err(|error| format!("key authorization: {error}"))?;
    if signed.key_id != key {
        return Err(format!(
            "Tempo ID approved the key {}, not this device's key {key}",
            signed.key_id
        ));
    }
    if signed.chain_id != CHAIN_ID {
        return Err(format!(
            "Tempo ID approved the key on chain {}, not Tempo mainnet ({CHAIN_ID})",
            signed.chain_id
        ));
    }
    Ok(signed)
}

pub fn install(payments: &Payments, record: &Record) -> Result<Value, String> {
    if record.expiry <= now() {
        return Err("the access key expired; sign in with Tempo ID again".into());
    }
    let signer: PrivateKeySigner = record
        .key
        .parse()
        .map_err(|error| format!("access key: {error}"))?;
    let account: Address = record
        .account
        .parse()
        .map_err(|error| format!("account: {error}"))?;
    let key = signer.address();
    let authorization = authorization(&record.authorization, key)?;
    if authorization.account.is_some_and(|owner| owner != account) {
        return Err("the key authorization names another account".into());
    }
    let wallet = TempoAccountsWallet::from_secp256k1(account, signer, Some(authorization));
    payments.install(TempoAccountsProvider::new(wallet), account, key, "tempo-id")?;
    Ok(payments.status())
}

/// The access key request: nanocode's MACH allowance and fee allowances, for a day, scoped to paying
/// MPP services (MACH transfers, the MACH swapper, and the TIP-1034 channel reserve).
pub fn key_request(key: Address, machine: Address, expiry: u64) -> Value {
    let machine = machine.to_string();
    let scopes = [
        json!({"address": machine, "selector": TRANSFER_WITH_MEMO}),
        json!({"address": machine, "recipients": [SWAPPER], "selector": APPROVE}),
        json!({"address": SWAPPER, "selector": SWAP_TO}),
    ]
    .into_iter()
    .chain(
        [OPEN, TOP_UP, REQUEST_CLOSE, WITHDRAW]
            .map(|selector| json!({"address": ESCROW, "selector": selector})),
    )
    .collect::<Vec<_>>();
    json!({
        "address": key.to_string(),
        "chainId": quantity(u128::from(CHAIN_ID)),
        "expiry": expiry,
        "keyType": "secp256k1",
        "limits": [
            {"limit": quantity(ALLOWANCE), "token": machine},
            {"limit": quantity(FEE_ALLOWANCE), "token": USDC.to_lowercase()},
            {"limit": quantity(FEE_ALLOWANCE), "token": PATH_USD},
        ],
        "scopes": scopes,
    })
}

/// The `POST /device/code` form for an access key request.
pub fn device_form(device: &str, request: &Value) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", CLIENT_ID.to_owned()),
        ("device", device.to_owned()),
        ("scope", SCOPE.to_owned()),
        (
            "key_authorization",
            URL_SAFE_NO_PAD.encode(request.to_string()),
        ),
        ("funding_hint", FUNDING_HINT.to_owned()),
        ("funding_token", FUNDING_TOKEN.to_owned()),
    ]
}

/// An `application/x-www-form-urlencoded` body (this crate's reqwest has no `form` feature).
fn form(pairs: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("form:").expect("a valid URL");
    url.query_pairs_mut().extend_pairs(pairs);
    url.query().unwrap_or_default().to_owned()
}

fn post(client: &reqwest::Client, path: &str, pairs: &[(&str, &str)]) -> reqwest::RequestBuilder {
    client
        .post(format!("{ISSUER}{path}"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form(pairs))
}

fn error_of(body: &Value) -> Option<&str> {
    body.get("error").and_then(Value::as_str)
}

fn description(body: &Value) -> String {
    body.get("error_description")
        .and_then(Value::as_str)
        .map_or_else(|| body.to_string(), str::to_owned)
}

pub async fn connect(host: &Host, meta: Meta) -> Result<Value, String> {
    let signer = PrivateKeySigner::random();
    let key_address = signer.address();
    let expiry = now() + LIFETIME.as_secs();
    let request = key_request(key_address, Payments::machine()?, expiry);
    let device = meta.name.unwrap_or_else(|| "nanocode".into());
    let client = reqwest::Client::builder()
        .timeout(POLL_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let fields = device_form(&device, &request);
    let fields = fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<Vec<_>>();
    let issued = post(&client, "/device/code", &fields)
        .send()
        .await
        .map_err(|error| format!("Tempo ID device authorization failed: {error}"))?;
    let status = issued.status();
    let body: Value = issued.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "Tempo ID refused the device authorization ({status}): {}",
            description(&body)
        ));
    }
    let field = |name: &str| body.get(name).and_then(Value::as_str).map(str::to_owned);
    let (Some(device_code), Some(user_code), Some(verification_uri)) = (
        field("device_code"),
        field("user_code"),
        field("verification_uri"),
    ) else {
        return Err(format!(
            "Tempo ID answered the device authorization without a device code: {body}"
        ));
    };
    let mut interval = Duration::from_secs(
        body.get("interval")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .max(1),
    );
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(APPROVAL_TIMEOUT.as_secs());
    host.emit(&json!({"wallet": {"prompt": {
        "user_code": user_code,
        "verification_uri": verification_uri,
        "verification_uri_complete": field("verification_uri_complete"),
        "expires_in": expires_in,
    }}}));
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(expires_in).min(APPROVAL_TIMEOUT);
    let mut failure: Option<String> = None;
    let token = loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(match failure {
                Some(failure) => format!(
                    "Timed out waiting for approval of code {user_code}; polling Tempo ID last failed: {failure}"
                ),
                None => format!("Timed out waiting for approval of code {user_code}."),
            });
        }
        tokio::time::sleep(interval).await;
        let polled = match post(
            &client,
            "/oauth2/token",
            &[
                ("grant_type", DEVICE_CODE_GRANT),
                ("client_id", CLIENT_ID),
                ("device_code", device_code.as_str()),
            ],
        )
        .send()
        .await
        {
            Ok(polled) => polled,
            Err(error) => {
                failure = Some(error.to_string());
                continue;
            }
        };
        let status = polled.status();
        let body: Value = match polled.json().await {
            Ok(body) => body,
            Err(error) if status.is_success() => {
                failure = Some(format!("reading Tempo ID's answer failed: {error}"));
                continue;
            }
            Err(_) => Value::Null,
        };
        if status.is_success() {
            break body;
        }
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            failure = Some(format!(
                "Tempo ID answered {status}: {}",
                description(&body)
            ));
            continue;
        }
        match error_of(&body) {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += Duration::from_secs(5),
            Some("access_denied") => {
                return Err("The sign-in was denied in Tempo ID.".into());
            }
            Some("expired_token") => {
                return Err("The device code expired before it was approved.".into());
            }
            Some("invalid_grant") => {
                return Err(format!(
                    "Tempo ID no longer accepts this device code: {}",
                    description(&body)
                ));
            }
            _ => {
                return Err(format!(
                    "Tempo ID answered {status}: {}",
                    description(&body)
                ));
            }
        }
    };
    let account = token
        .get("account")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Tempo ID returned no account: {token}"))?;
    let authorization = token
        .get("key_authorization")
        .cloned()
        .ok_or("Tempo ID returned no key authorization")?;
    let record = Record {
        account: account.to_owned(),
        key: hex(&signer.credential().to_bytes()),
        key_address: key_address.to_string(),
        expiry,
        authorization,
    };
    install(&host.payments, &record)?;
    serde_json::to_value(record).map_err(|error| error.to_string())
}

/// Tempo ID's deposit page for `account`, which offers Buy MACH and the other ways to add funds.
pub fn funding_url(account: &str) -> String {
    let mut url = reqwest::Url::parse(&format!("{ISSUER}/deposit")).expect("a valid URL");
    url.query_pairs_mut()
        .append_pair("address", account)
        .append_pair("chain", &CHAIN_ID.to_string())
        .append_pair("funding_hint", FUNDING_HINT)
        .append_pair("funding_token", FUNDING_TOKEN);
    url.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "0x9a073914db0d149375ac2b7fbb72c68c0780fb5f";

    /// A token response's key authorization in the shape Tempo ID returns: the Tempo node's RPC
    /// format, with the scopes as `allowedCalls`.
    fn signed(key: Address, chain: u64) -> Value {
        json!({
            "account": ACCOUNT,
            "allowedCalls": [
                {"target": SWAPPER, "selectorRules": [{"selector": SWAP_TO}]},
                {"target": ESCROW, "selectorRules": [{"selector": OPEN}, {"selector": TOP_UP}]},
            ],
            "chainId": quantity(u128::from(chain)),
            "expiry": "0x6acdc1b4",
            "keyId": key.to_string(),
            "keyType": "secp256k1",
            "limits": [{"token": PATH_USD, "limit": "0x186a0"}],
            "signature": {
                "type": "secp256k1",
                "r": "0x1b7f8e2a9b6f4f7e1c3d5a7b9c1d3e5f7a9b1c3d5e7f9a1b3c5d7e9f1a3b5c7d",
                "s": "0x2c8f9e3b0c7f5f8e2d4e6b8c0d2e4f6a8b0c2d4e6f8a0b2c4d6e8f0a2b4c6d8e",
                "yParity": "0x1",
            },
        })
    }

    #[test]
    fn reads_tempo_id_key_authorization_with_scopes() {
        let key = PrivateKeySigner::random().address();
        let authorization = authorization(&signed(key, CHAIN_ID), key).expect("reads");
        assert_eq!(authorization.key_id, key);
        assert_eq!(authorization.account, Some(ACCOUNT.parse().unwrap()));
        let calls = authorization
            .allowed_calls
            .as_ref()
            .expect("keeps the scopes");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].selector_rules().len(), 2);
        assert_eq!(authorization.limits.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn refuses_another_key_or_chain() {
        let key = PrivateKeySigner::random().address();
        let other = PrivateKeySigner::random().address();
        assert!(authorization(&signed(other, CHAIN_ID), key).is_err());
        assert!(authorization(&signed(key, 42431), key).is_err());
    }

    #[test]
    fn device_form_carries_the_access_key_request() {
        let key = PrivateKeySigner::random().address();
        let request = key_request(key, Payments::machine().unwrap(), 1_800_000_000);
        let form = device_form("nanocode for macOS", &request);
        let get = |name: &str| {
            form.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("client_id").as_deref(), Some("cli:nanocode"));
        let decoded: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(get("key_authorization").unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(decoded, request);
        assert_eq!(decoded["scopes"].as_array().map(Vec::len), Some(7));
        assert_eq!(decoded["limits"].as_array().map(Vec::len), Some(3));
    }

    /// Asks the live id.tempo.xyz for a device code with this crate's request and polls once; run with
    /// `--ignored`. Approving the code needs a person's passkey, so it stops at `authorization_pending`.
    #[test]
    #[ignore = "reaches id.tempo.xyz"]
    fn live_device_code_is_issued() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        nanocode::codex::transport::install_default_rustls_crypto_provider();
        nanocode::codex::transport::install_default_rustls_crypto_provider();
        runtime.block_on(async {
            let client = reqwest::Client::new();
            let key = PrivateKeySigner::random().address();
            let request = key_request(
                key,
                Payments::machine().unwrap(),
                now() + LIFETIME.as_secs(),
            );
            let fields = device_form("nanocode test", &request);
            let fields = fields
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect::<Vec<_>>();
            let issued = post(&client, "/device/code", &fields).send().await.unwrap();
            let status = issued.status();
            let body: Value = issued.json().await.unwrap();
            assert!(status.is_success(), "{status}: {body}");
            assert!(body["user_code"].is_string(), "{body}");
            let device_code = body["device_code"].as_str().unwrap();
            let polled = post(
                &client,
                "/oauth2/token",
                &[
                    ("grant_type", DEVICE_CODE_GRANT),
                    ("client_id", CLIENT_ID),
                    ("device_code", device_code),
                ],
            )
            .send()
            .await
            .unwrap();
            let body: Value = polled.json().await.unwrap();
            assert_eq!(error_of(&body), Some("authorization_pending"), "{body}");
        });
    }

    #[test]
    fn funding_url_opens_tempo_id_deposit() {
        assert_eq!(
            funding_url(ACCOUNT),
            format!(
                "https://id.tempo.xyz/deposit?address={ACCOUNT}&chain=4217&funding_hint=2&funding_token=MACH"
            )
        );
    }
}
