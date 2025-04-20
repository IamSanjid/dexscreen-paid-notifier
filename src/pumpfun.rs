pub const FRONTEND_WS_URL: &str =
    "wss://frontend-api-v3.pump.fun/socket.io/?EIO=4&transport=websocket";

pub fn get_token_name<'a>(json: &'a str) -> Option<&'a str> {
    const NAME_FIELD: &str = "\"name\":";
    if let Some(pos) = json.find(NAME_FIELD) {
        let (_, next_half) = json.split_at(pos + NAME_FIELD.len());
        let next_half = &next_half[1..];
        if let Some(pos) = next_half.find('"').or_else(|| next_half.find('}')) {
            let name = &next_half[..pos];
            return Some(name);
        }
    }
    return None;
}

pub fn get_token_mint<'a>(json: &'a str) -> Option<&'a str> {
    const MINT_FIELD: &str = "\"mint\":";
    if let Some(pos) = json.find(MINT_FIELD) {
        let (_, next_half) = json.split_at(pos + MINT_FIELD.len());
        let next_half = &next_half[1..];
        if let Some(pos) = next_half.find('"').or_else(|| next_half.find('}')) {
            let mint = &next_half[..pos];
            return Some(mint);
        }
    }
    return None;
}

#[allow(dead_code)]
pub fn get_token_created_timestamp<'a>(json: &'a str) -> Option<u64> {
    const CREATED_FIELD: &str = "\"created_timestamp\":";
    if let Some(pos) = json.find(CREATED_FIELD) {
        let (_, next_half) = json.split_at(pos + CREATED_FIELD.len());
        if let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) {
            let created = &next_half[..pos];
            return created.parse::<u64>().ok();
        }
    }
    return None;
}

#[allow(dead_code)]
pub fn get_token_market_cap<'a>(json: &'a str) -> Option<f64> {
    const MARKET_CAP_FIELD: &str = "\"market_cap\":";
    if let Some(pos) = json.find(MARKET_CAP_FIELD) {
        let (_, next_half) = json.split_at(pos + MARKET_CAP_FIELD.len());
        if let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) {
            let market_cap = &next_half[..pos];
            return market_cap.parse::<f64>().ok();
        }
    }
    return None;
}

pub fn get_token_usd_market_cap<'a>(json: &'a str) -> Option<f64> {
    const USD_MARKET_CAP_FIELD: &str = "\"usd_market_cap\":";
    if let Some(pos) = json.find(USD_MARKET_CAP_FIELD) {
        let (_, next_half) = json.split_at(pos + USD_MARKET_CAP_FIELD.len());
        if let Some(pos) = next_half.find(',').or_else(|| next_half.find('}')) {
            let market_cap = &next_half[..pos];
            return market_cap.parse::<f64>().ok();
        }
    }
    return None;
}
