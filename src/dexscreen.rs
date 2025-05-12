use crate::common::{Token, calculate_time_difference};
use reqwest;
use simd_json::{
    self,
    base::{ValueAsArray, ValueAsObject, ValueAsScalar},
};
use std::time::Duration;
use tokio;

#[allow(unused_imports)]
use crate::{debug_eprintln, debug_println};
#[allow(unused_imports)]
use http::header;

#[allow(dead_code)]
pub fn get_token_created_timestamp<'a>(json: &'a str) -> Option<u64> {
    const CREATED_TIMESTAMP_FIELD: &str = "\"pairCreatedAt\":";
    if let Some(pos) = json.find(CREATED_TIMESTAMP_FIELD) {
        let (_, next_half) = json.split_at(pos + CREATED_TIMESTAMP_FIELD.len());
        if let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) {
            let created_timestamp = &next_half[..pos];
            return created_timestamp.parse::<u64>().ok();
        }
    }
    return None;
}

pub fn get_token_market_cap<'a>(json: &'a str) -> Option<f64> {
    const MARKET_CAP_FIELD: &str = "\"marketCap\":";
    if let Some(pos) = json.find(MARKET_CAP_FIELD) {
        let (_, next_half) = json.split_at(pos + MARKET_CAP_FIELD.len());
        if let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) {
            let market_cap = &next_half[..pos];
            return market_cap.parse::<f64>().ok();
        }
    }
    return None;
}

pub fn get_token<'a>(json: &'a str) -> Option<Token> {
    const BASE_TOKEN_FIELD: &str = "\"baseToken\":{";
    const BASE_TOKEN_ADDRESS_FIELD: &str = "\"address\":";
    const BASE_TOKEN_NAME_FIELD: &str = "\"name\":";

    let Some(pos) = json.find(BASE_TOKEN_FIELD) else {
        return None;
    };
    let (_, base_token_half) = json.split_at(pos + BASE_TOKEN_FIELD.len());

    // address
    let Some(pos) = base_token_half.find(BASE_TOKEN_ADDRESS_FIELD) else {
        return None;
    };
    let (_, next_half) = base_token_half.split_at(pos + BASE_TOKEN_ADDRESS_FIELD.len());
    let next_half = &next_half[1..]; // skip the first `"`
    let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) else {
        return None;
    };
    let address = &next_half[..pos - 1]; // skip the last `"`

    // name
    let Some(pos) = next_half.find(BASE_TOKEN_NAME_FIELD) else {
        return None;
    };
    let (_, next_half) = next_half.split_at(pos + BASE_TOKEN_NAME_FIELD.len());
    let next_half = &next_half[1..]; // skip the first `"`
    let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) else {
        return None;
    };
    let name = &next_half[..pos - 1]; // skip the last `"`

    // market cap
    let Some(usd_market_cap) = get_token_market_cap(json) else {
        debug_eprintln!("Failed to parse market cap: {}", json);
        return None;
    };

    return Some(Token {
        mint: address.to_string(),
        name: name.to_string(),
        usd_market_cap,
    });
}

#[derive(Debug)]
pub enum DexscreenError {
    TooManyRequests,
    ReqwestError(reqwest::Error),
    SimdJsonError(simd_json::Error),
    Other(String),
}

impl std::fmt::Display for DexscreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DexscreenError::TooManyRequests => write!(f, "Too many requests"),
            DexscreenError::ReqwestError(e) => write!(f, "Reqwest error: {e}"),
            DexscreenError::SimdJsonError(e) => write!(f, "SimdJson error: {e}"),
            DexscreenError::Other(e) => write!(f, "Other error: {e}"),
        }
    }
}

impl std::error::Error for DexscreenError {}

const CHANNEL_ID: &str = "solana";
const PAID_TIMEOUT_SECS: u64 = 5 * 60; // 5 minutes
#[cfg(feature = "batch_requests")]
pub fn paid_request(token: &Token, client: &reqwest::Client) -> reqwest::RequestBuilder {
    let url = format!(
        "https://api.dexscreener.com/orders/v1/{}/{}",
        CHANNEL_ID, token.mint
    );

    return client.get(&url);
}

#[cfg(feature = "batch_requests")]
pub async fn is_response_paid(response: reqwest::Response) -> Result<bool, DexscreenError> {
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(DexscreenError::TooManyRequests);
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| DexscreenError::ReqwestError(e))?;

    const PROCESSING: &[u8] = b"processing";
    if bytes
        .windows(PROCESSING.len())
        .any(|window| window == PROCESSING)
    {
        return Ok(true);
    }
    let mut bytes = bytes.to_vec();
    let parsed =
        simd_json::to_borrowed_value(&mut bytes).map_err(|e| DexscreenError::SimdJsonError(e))?;
    let orders_value = parsed.as_array().ok_or(DexscreenError::Other)?;
    if orders_value.is_empty() {
        return Ok(false);
    }
    for order_value in orders_value {
        let Some(order) = order_value.as_object() else {
            continue;
        };

        let Some(order_type) = order.get("type") else {
            continue;
        };

        if order_type.as_str() != Some("tokenProfile") {
            continue;
        }

        let Some(order_status) = order.get("status") else {
            continue;
        };
        if order_status.as_str() != Some("approved") {
            continue;
        }

        let Some(payment_timestamp) = order.get("paymentTimestamp").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(diff) = calculate_time_difference(payment_timestamp) else {
            continue;
        };

        if diff.as_secs() < PAID_TIMEOUT_SECS {
            return Ok(true);
        }
    }

    return Ok(false);
}

pub async fn try_check_if_paid(
    token: &Token,
    client: &reqwest::Client,
) -> Result<bool, DexscreenError> {
    let url = format!(
        "https://api.dexscreener.com/orders/v1/{}/{}",
        CHANNEL_ID, token.mint
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| DexscreenError::ReqwestError(e))?;

    #[cfg(debug_assertions)]
    if response.status() != reqwest::StatusCode::OK
        && response.status() != reqwest::StatusCode::CREATED
    {
        let Ok(res) = client
            .get("http://httpbin.org/ip")
            .header(header::USER_AGENT, "")
            .send()
            .await
        else {
            return Err(DexscreenError::Other("Failed to fetch IP".to_owned()));
        };
        let Ok(ip_body) = res.text().await else {
            debug_eprintln!("Failed to fetch IP");
            return Err(DexscreenError::Other("Failed to fetch IP".to_owned()));
        };

        let Ok(res) = client
            .get("http://httpbin.org/headers")
            .header(header::USER_AGENT, "")
            .send()
            .await
        else {
            debug_eprintln!("Failed to fetch HEADERS");
            return Err(DexscreenError::Other("Failed to fetch HEADERS".to_owned()));
        };
        let Ok(body) = res.text().await else {
            debug_eprintln!("Failed to fetch HEADERS");
            return Err(DexscreenError::Other("Failed to fetch HEADERS".to_owned()));
        };

        debug_println!("Ip check: {}", ip_body);
        debug_println!("Headers check: {}", body);

        debug_eprintln!(
            "Failed to fetch orders: {} - user-agent: {}",
            response.status(),
            ""
        );
        debug_println!("Response: {:?}", response.headers());
        return Err(DexscreenError::Other("Failed to fetch orders".to_owned()));
    }

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(DexscreenError::TooManyRequests);
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| DexscreenError::ReqwestError(e))?;

    const PROCESSING: &[u8] = b"processing";
    if bytes
        .windows(PROCESSING.len())
        .any(|window| window == PROCESSING)
    {
        return Ok(true);
    }
    let mut bytes = bytes.to_vec();
    let parsed =
        simd_json::to_borrowed_value(&mut bytes).map_err(|e| DexscreenError::SimdJsonError(e))?;
    let orders_value = parsed
        .as_array()
        .ok_or(DexscreenError::Other("Parsed array error".to_owned()))?;
    if orders_value.is_empty() {
        return Ok(false);
    }
    for order_value in orders_value {
        let Some(order) = order_value.as_object() else {
            continue;
        };

        let Some(order_type) = order.get("type") else {
            continue;
        };

        if order_type.as_str() != Some("tokenProfile") {
            continue;
        }

        let Some(order_status) = order.get("status") else {
            continue;
        };
        if order_status.as_str() != Some("approved") {
            continue;
        }

        let Some(payment_timestamp) = order.get("paymentTimestamp").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(diff) = calculate_time_difference(payment_timestamp) else {
            continue;
        };

        if diff.as_secs() < PAID_TIMEOUT_SECS {
            return Ok(true);
        }
    }

    return Ok(false);
}

pub async fn check_if_paid(token: &Token, client: &reqwest::Client) -> bool {
    let result = try_check_if_paid(token, client).await;
    match result {
        Ok(paid) => paid,
        Err(e) => {
            #[cfg(debug_assertions)]
            {
                let Ok(res) = client
                    .get("http://httpbin.org/ip")
                    .header(header::USER_AGENT, "")
                    .send()
                    .await
                else {
                    return false;
                };
                let Ok(ip_body) = res.text().await else {
                    debug_println!("Failed to fetch IP");
                    return false;
                };
                debug_println!("Ip check: {}", ip_body);
            }
            if matches!(e, DexscreenError::TooManyRequests) {
                debug_println!("Too many requests, sleeping for 5 seconds");
                tokio::time::sleep(Duration::from_secs(5)).await;
            } else {
                debug_println!("Error: {}", e);
            }

            return false;
        }
    }
}
