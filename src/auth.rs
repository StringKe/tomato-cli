use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::time::{Duration, sleep};

use crate::api::{self, Client, HOST};
use crate::model::User;

pub struct QrTicket {
    pub token: String,
    pub payload: String,
}

pub async fn start_qr(client: &Client) -> Result<QrTicket> {
    let next = urlencoding_next();
    let url = format!("{HOST}/passport/web/get_qrcode/?next={next}&{}", api::passport_query());
    let json: Value = client.get_json(&url, &[]).await?;
    let data = json.get("data").cloned().unwrap_or(json);
    if let Some(code) = data.get("error_code").and_then(Value::as_i64)
        && code != 0
    {
        return Err(anyhow!("{}", data.get("description").and_then(Value::as_str).unwrap_or("获取二维码失败")));
    }
    let token = data.get("token").or_else(|| data.get("qr_token")).and_then(Value::as_str).unwrap_or("").to_string();
    if token.is_empty() {
        return Err(anyhow!("二维码缺少 token"));
    }
    let qr_text = data.get("qrcode_index_url").and_then(Value::as_str).unwrap_or("").to_string();
    let payload = if qr_text.is_empty() { token.clone() } else { qr_text };
    if payload.is_empty() {
        return Err(anyhow!("二维码缺少内容"));
    }
    Ok(QrTicket { token, payload })
}

pub async fn poll_qr(client: &Client, ticket: &QrTicket) -> Result<String> {
    let next = urlencoding_next();
    let url = format!("{HOST}/passport/web/check_qrconnect/?next={next}&token={}&{}", urlencoding::encode(&ticket.token), api::passport_query());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err(anyhow!("二维码已过期"));
        }
        let json = match client.get_json(&url, &[]).await {
            Ok(v) => v,
            Err(_) => {
                sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let data = json.get("data").cloned().unwrap_or(json);
        let status = data.get("status").and_then(Value::as_str).unwrap_or("");
        let redirect = data
            .get("redirect_url")
            .or_else(|| data.get("redirectUrl"))
            .or_else(|| data.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !redirect.is_empty() || matches_confirm(status) {
            return Ok(if redirect.is_empty() { format!("{HOST}/") } else { redirect.to_string() });
        }
        if let Some(code) = data.get("error_code").and_then(Value::as_i64)
            && code != 0
        {
            return Err(anyhow!("二维码状态异常 {code}"));
        }
        sleep(Duration::from_secs(2)).await;
    }
}

pub async fn finalize(client: &Client, redirect: &str) -> Result<User> {
    let _ = client.request("GET", redirect, &[("Accept", "text/html,application/json")], None).await;
    let _ = client.request("GET", &format!("{HOST}/"), &[("Accept", "text/html,application/json")], None).await;
    client.user_info().await?.ok_or_else(|| anyhow!("扫码已确认，但未能建立会话"))
}

fn matches_confirm(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("confirm") || s.contains("success") || s.contains("done") || s == "ok"
}

fn urlencoding_next() -> String {
    urlencoding::encode("https://fanqienovel.com/").into_owned()
}
