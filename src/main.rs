use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use chrono::{DateTime, Local};
use fast_websocket_client::{OpCode, WebSocketClientError, base_client};
use http::{HeaderMap, HeaderValue, header};
#[cfg(feature = "notify")]
use notify_rust::Notification;
use reqwest;
#[cfg(feature = "notify")]
use rodio::{Decoder, OutputStream, source::Source};
use tokio::{self, sync::broadcast, task::JoinHandle};

use dexscreen_paid_notifier::common::Token;
use dexscreen_paid_notifier::config::get_config;
use dexscreen_paid_notifier::dexscreen;
use dexscreen_paid_notifier::pumpfun;
use dexscreen_paid_notifier::{debug_eprintln, debug_println};

#[derive(Clone, Debug)]
struct TokenCheckRequest {
    token: Token,
    proxy: &'static str,
}

struct TokenCheck {
    token: Token,
    proxy_id: usize,
}

impl TokenCheck {
    fn from_request(tr: TokenCheckRequest, id: usize) -> Self {
        Self {
            token: tr.token,
            proxy_id: id,
        }
    }
}

fn create_client(
    proxy: Option<&str>,
    origin: Option<&str>,
) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
    if let Some(origin) = origin {
        headers.append(header::ORIGIN, HeaderValue::from_str(origin)?);
    }
    let mut client_builder = reqwest::Client::builder();
    if let Some(proxy) = proxy {
        client_builder = client_builder.proxy(reqwest::Proxy::all(proxy)?);
    }
    //headers.append("user-agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36"));
    return Ok(client_builder
        .default_headers(headers)
        .http1_ignore_invalid_headers_in_responses(true)
        .http1_title_case_headers()
        .danger_accept_invalid_certs(true)
        .build()?);
}

type PaidNotification = (DateTime<Local>, Token);
async fn paid_notifications(mut rx: broadcast::Receiver<PaidNotification>) {
    #[cfg(feature = "notify")]
    let (stream_handle, source) = {
        let file =
            std::io::BufReader::new(std::fs::File::open("beep.wav").expect("beep.wav not found"));
        let source = Decoder::new(file).unwrap().buffered();
        let (_stream, stream_handle) =
            OutputStream::try_default().expect("Failed to open audio stream");

        (stream_handle, source)
    };
    #[cfg(feature = "notify")]
    let source = &source;

    while let Ok((now, token)) = rx.recv().await {
        let now = now.format("%H:%M:%S");
        let pumpfun_url = format!(
            "https://pump.fun/board?include-nsfw=true&q={}&coins_sort=market_cap",
            token.mint
        );
        println!(
            "[{}]PAID: {} - {} is paid! ({})",
            now, token.name, token.mint, pumpfun_url
        );

        #[cfg(feature = "notify")]
        {
            _ = Notification::new()
                .summary("Token is paid!")
                .body(
                    format!(
                        "[{}] {} - {} is paid! ({})",
                        now, token.name, token.mint, pumpfun_url
                    )
                    .as_str(),
                )
                .appname("Dexscreen Paid Notifier")
                .timeout(5000)
                .show();

            let cloned_source = source.clone();
            if stream_handle
                .play_raw(cloned_source.convert_samples())
                .is_ok()
            {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn check_dexscreen_paid(
    mut rx: broadcast::Receiver<TokenCheckRequest>,
    tx: broadcast::Sender<PaidNotification>,
) {
    let mut check_tokens = Vec::new();

    let mut proxies_map = HashMap::new();
    let mut proxies = Vec::new();

    let mut recent_paid = HashSet::new();
    let mut timeout_5min: Option<JoinHandle<()>> = None;

    #[cfg(feature = "batch_requests")]
    let mut batch_reqs = Vec::new();

    loop {
        let entry = tokio::time::timeout(Duration::from_millis(1), rx.recv()).await;
        match entry {
            Ok(Ok(tc_req)) => {
                let (token_check, client) = if let Some(id) = proxies_map.get(tc_req.proxy) {
                    (TokenCheck::from_request(tc_req, *id), unsafe {
                        proxies.get_unchecked(*id)
                    })
                } else {
                    let client = create_client(Some(tc_req.proxy), None).unwrap();
                    proxies_map.insert(tc_req.proxy, proxies.len());
                    let token_check = TokenCheck::from_request(tc_req, proxies.len());
                    proxies.push(client);

                    let client = unsafe { proxies.get_unchecked(token_check.proxy_id) };
                    (token_check, client)
                };

                let token = &token_check.token;
                if dexscreen::check_if_paid(token, client).await {
                    let now = Local::now();
                    _ = tx.send((now, token.clone()));

                    recent_paid.insert(token.mint.clone());
                }

                check_tokens.push(token_check);
            }
            Ok(Err(_)) => break,
            Err(_) => {} // timeout...
        }

        #[allow(unused_variables)]
        for (i, tc) in check_tokens.iter().enumerate() {
            let token = &tc.token;
            if recent_paid.contains(&token.mint) {
                continue;
            }

            let client = unsafe { proxies.get_unchecked(tc.proxy_id) };

            #[cfg(feature = "batch_requests")]
            {
                batch_reqs.push((
                    i,
                    tokio::spawn(dexscreen::paid_request(token, client).send()),
                ));

                if batch_reqs.len() >= 5 {
                    for (i, req) in batch_reqs.drain(..) {
                        let result = req.await;
                        match result {
                            Ok(Ok(resp)) => {
                                if let Ok(paid) = dexscreen::is_response_paid(resp).await {
                                    if paid {
                                        let token = unsafe { &check_tokens.get_unchecked(i).token };
                                        let now = Local::now();
                                        _ = tx.send((now, token.clone()));

                                        recent_paid.insert(token.mint.clone());
                                    }
                                }
                            }
                            Ok(Err(_)) => {}
                            Err(_) => {}
                        }
                    }
                }
            }

            #[cfg(not(feature = "batch_requests"))]
            if dexscreen::check_if_paid(token, client).await {
                let now = Local::now();
                _ = tx.send((now, token.clone()));

                recent_paid.insert(token.mint.clone());
            }
        }

        #[cfg(feature = "batch_requests")]
        if batch_reqs.len() > 0 {
            for (i, req) in batch_reqs.drain(..) {
                let result = req.await;
                match result {
                    Ok(Ok(resp)) => {
                        if let Ok(paid) = dexscreen::is_response_paid(resp).await {
                            if paid {
                                let token = unsafe { &check_tokens.get_unchecked(i).token };
                                let now = Local::now();
                                _ = tx.send((now, token.clone()));

                                recent_paid.insert(token.mint.clone());
                            }
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(_) => {}
                }
            }
        }

        if !recent_paid.is_empty() {
            if let Some(timeout) = &timeout_5min {
                if timeout.is_finished() {
                    recent_paid.clear();
                    timeout_5min = Some(tokio::spawn(tokio::time::sleep(Duration::from_secs(
                        5 * 60,
                    ))));
                }
            } else {
                timeout_5min = Some(tokio::spawn(tokio::time::sleep(Duration::from_secs(
                    5 * 60,
                ))));
            }
        }
    }
}

#[derive(Debug, Clone)]
enum CheckToken {
    Pumpfun(String),
    Dexscreen(Token),
}

async fn handle_token_checkers(mut rx: broadcast::Receiver<CheckToken>) {
    const PUMPFUN_TYPE: &'static str = "42[\"tradeCreated\",";
    let proxies = get_config().proxies();

    let mut seen_mints = HashSet::new();

    let (n_tx, n_rx) = broadcast::channel::<PaidNotification>(32);
    tokio::spawn(paid_notifications(n_rx));

    let mut checkers = Vec::new();
    for _ in 0..get_config().checkers_count() {
        let (tx, rx) = broadcast::channel::<TokenCheckRequest>(256);
        tokio::spawn(check_dexscreen_paid(rx, n_tx.clone()));
        checkers.push(tx);
    }

    let mut checkers_index = 0;
    let mut proxy_index = 0;

    while let Ok(message) = rx.recv().await {
        match message {
            CheckToken::Pumpfun(message) => {
                if message.starts_with(PUMPFUN_TYPE) {
                    let message = message.trim_start_matches(PUMPFUN_TYPE);
                    if let Some(pos) = message.rfind(']') {
                        let json = &message[..pos];
                        //println!("Parsed JSON: {}", json);
                        let Some(usd_market_cap) = pumpfun::get_token_usd_market_cap(json) else {
                            debug_eprintln!("Failed to parse market cap: {message}");
                            continue;
                        };
                        if usd_market_cap < 10000.0 {
                            continue;
                        }
                        let mint = pumpfun::get_token_mint(json)
                            .expect(format!("Failed to parse mint: {message}").as_str());
                        if seen_mints.contains(mint) {
                            continue;
                        }
                        let name = pumpfun::get_token_name(json)
                            .expect(format!("Failed to parse name: {message}").as_str());

                        debug_println!(
                            "Pumpfun - Checking: '{}'(${}) - {}",
                            name,
                            usd_market_cap,
                            mint
                        );

                        seen_mints.insert(mint.to_string());

                        let checker = unsafe { checkers.get_unchecked(checkers_index) };
                        checkers_index = (checkers_index + 1) % checkers.len();

                        let proxy = unsafe { proxies.get_unchecked(proxy_index) };
                        proxy_index = (proxy_index + 1) % proxies.len();

                        _ = checker.send(TokenCheckRequest {
                            token: Token {
                                mint: mint.to_string(),
                                name: name.to_string(),
                                usd_market_cap,
                            },
                            proxy,
                        });
                    }
                }
            }
            CheckToken::Dexscreen(token) => {
                if seen_mints.contains(&token.mint) {
                    continue;
                }

                debug_println!(
                    "Dexscreen - Checking: '{}'(${}) - {}",
                    token.name,
                    token.usd_market_cap,
                    token.mint
                );

                seen_mints.insert(token.mint.clone());

                let checker = unsafe { checkers.get_unchecked(checkers_index) };
                checkers_index = (checkers_index + 1) % checkers.len();

                let proxy = unsafe { proxies.get_unchecked(proxy_index) };
                proxy_index = (proxy_index + 1) % proxies.len();

                _ = checker.send(TokenCheckRequest { token, proxy });
            }
        }
    }
}

async fn handle_pumpfun_ws(
    client: &mut base_client::Online,
    tx: &broadcast::Sender<CheckToken>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let frame = client.receive_frame().await?;
    match frame.opcode {
        OpCode::Text => {
            let text = frame.payload.as_ref();
            if text.starts_with(b"0{") {
                client.send_string("40").await?;
            } else if text.starts_with(b"2") {
                client.send_string("3").await?;
            } else {
                if let Ok(text) = simdutf8::basic::from_utf8(&frame.payload.as_ref()) {
                    let text = text.to_owned(); // copy and send to other thread
                    _ = tx.send(CheckToken::Pumpfun(text)); // don't care let's continue...
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn send_dexscreen_tokens_to_checker(
    text: String,
    tx: &broadcast::Sender<CheckToken>,
) -> Result<(), ()> {
    const START_MARKER: &str = "\"pairs\":[{";
    const END_MARKER: &str = "],\"pairsCount\":";

    let Some(pos) = text.find(START_MARKER) else {
        #[cfg(debug_assertions)]
        {
            eprintln!("Failed to find start marker!");
            _ = std::fs::write("dexscreen_debug.html", text);
        }
        return Err(());
    };
    let (_, next_half) = text.split_at(pos + START_MARKER.len() - 2); // include the `[{`
    let Some(pos) = next_half.find(END_MARKER) else {
        #[cfg(debug_assertions)]
        {
            eprintln!("Failed to find end marker!");
            _ = std::fs::write("dexscreen_debug.html", text);
        }
        return Err(());
    };
    let pairs_json = &next_half[..pos + 1]; // include the `]`
    for pair in pairs_json.split("},{") {
        let Some(token) = dexscreen::get_token(pair) else {
            continue;
        };
        if token.usd_market_cap < 10000.0 {
            continue;
        }
        _ = tx.send(CheckToken::Dexscreen(token));
    }

    Ok(())
}

async fn fetch_tokens_from_dexscreen(tx: broadcast::Sender<CheckToken>) {
    let mut headers = HeaderMap::new();
    headers.append(
        header::USER_AGENT,
        HeaderValue::from_static(get_config().user_agent()),
    );
    headers.append(
        header::ORIGIN,
        HeaderValue::from_static("https://dexscreener.com"),
    );
    headers.append(
        header::COOKIE,
        HeaderValue::from_static(get_config().dexscreen_cookie()),
    );

    let config = get_config();

    let client = reqwest::ClientBuilder::new()
        .default_headers(headers)
        .build()
        .expect("Failed to create client");

    let mut end_page = 1;
    loop {
        let mut fetch_tasks = Vec::new();

        debug_println!("Fetching dexscreener page from 1 to {}", end_page);

        for page in 1..=end_page {
            fetch_tasks.extend(
                config
                    .dexscreen_fetch_filters()
                    .iter()
                    .map(|filter| {
                        format!("https://dexscreener.com/solana/pumpfun/page-{page}?{filter}")
                    })
                    .map(|url| {
                        let req = client.get(url);
                        tokio::spawn(async {
                            let Ok(resp) = req.send().await else {
                                return None;
                            };
                            let Ok(text) = resp.text().await else {
                                return None;
                            };

                            return Some(text);
                        })
                    }),
            );
        }

        for task in fetch_tasks {
            let Ok(Some(text)) = task.await else {
                continue;
            };
            // since all the pages checking tasks are spawned sequentially(1, 2, 3, ...),
            // we can just break the loop if we get an error, since we won't gonna get any
            // more valid tokens from the later pages.
            if send_dexscreen_tokens_to_checker(text, &tx).is_err() {
                debug_eprintln!("There's nothing in the later pages!");
                break;
            }
        }
        end_page = end_page % config.dexscreen_fetch_max_pages();
        end_page += 1;
        tokio::time::sleep(Duration::from_secs(config.dexscreen_fetch_timeout())).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    debug_println!("Debug mode!");
    println!("Starting Dexscreen paid notifier...\n{:#?}", get_config());

    #[cfg(debug_assertions)]
    {
        println!("Check proxies in config.json? y/N(default: N)");
        let mut text = String::new();
        if std::io::stdin().read_line(&mut text).is_err() {
            text = "N".to_string();
        }
        let ans = text.trim().to_lowercase();
        match ans.as_str() {
            "y" | "yes" => {
                println!("Checking proxies...");
                let proxies = get_config().proxies();
                let mut checks = vec![];
                for (i, proxy) in proxies.iter().enumerate() {
                    let client = create_client(Some(proxy), None).unwrap();
                    checks.push((
                        i,
                        tokio::spawn(
                            client
                                .get("http://httpbin.org/ip")
                                .header(header::USER_AGENT, "")
                                .send(),
                        ),
                    ));
                }
                for (i, task) in checks {
                    let proxy = unsafe { proxies.get_unchecked(i) };
                    let Ok(Ok(resp)) = task.await else {
                        println!("Failed to check proxy {}!", proxy);
                        continue;
                    };

                    let Ok(text) = resp.text().await else {
                        println!("Failed to read response from proxy {}!", proxy);
                        continue;
                    };

                    if text.find("\"origin\":").is_none() {
                        println!("Proxy {} is not working!\n{}", proxy, text);
                    }
                }

                println!("Proxy check finished! Press any key to continue...");
                _ = std::io::stdin().read_line(&mut text);
            }
            _ => {
                println!("Skipping proxy check...");
            }
        }
    }

    let mut offline = base_client::Offline::new();
    offline.add_header(
        header::COOKIE,
        HeaderValue::from_static(get_config().pumpfun_cookie()),
    );
    offline.add_header(
        header::HOST,
        HeaderValue::from_static("frontend-api-v3.pump.fun"),
    );
    offline.add_header(header::ORIGIN, HeaderValue::from_static("https://pump.fun"));
    offline.add_header(
        header::USER_AGENT,
        HeaderValue::from_static(get_config().user_agent()),
    );
    offline.set_auto_pong(true);
    offline.set_auto_close(false);

    let client = offline
        .connect(pumpfun::FRONTEND_WS_URL)
        .await
        .map_err(|error| {
            WebSocketClientError::ConnectionError(format!("Failed to connect: {}", error))
        })?;

    let (tx, rx) = broadcast::channel::<CheckToken>(256 * 2);

    let t = tokio::spawn(handle_token_checkers(rx));
    tokio::spawn(fetch_tokens_from_dexscreen(tx.clone()));

    tokio::spawn(async move {
        let mut client = client;
        loop {
            if let Err(_) = handle_pumpfun_ws(&mut client, &tx).await {
                break;
            }
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    t.await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use tokio;

    #[tokio::test]
    async fn test_check_if_dexscreen_paid() {
        // Mock token data
        let token = Token {
            mint: "FnTEh9q7m2qj8aRm7rCuNzKh5PSgftqrqFszEGLFpump".to_string(),
            name: "Prophecy".to_string(),
            usd_market_cap: 10000.0,
        };

        let client = Client::new();
        let result = dexscreen::check_if_paid(&token, &client).await;
        assert!(result, "The token should be marked as paid.");
    }
}
