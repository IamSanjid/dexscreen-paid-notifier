use simd_json;
use simd_json::base::{ValueAsScalar, ValueIntoArray, ValueIntoObject, ValueIntoString};
use std::sync::OnceLock;
use std::{env, fs};

#[derive(Debug)]
pub struct Config {
    pumpfun_cookie: &'static str,
    dexscreen_cookie: &'static str,
    proxies: Vec<&'static str>,
    checkers_count: usize,
    dexscreen_fetch_filters: Vec<&'static str>,
    dexscreen_fetch_max_pages: usize,
    dexscreen_fetch_timeout: u64,
}

impl Config {
    pub fn pumpfun_cookie(&'static self) -> &'static str {
        self.pumpfun_cookie
    }

    pub fn dexscreen_cookie(&'static self) -> &'static str {
        self.dexscreen_cookie
    }

    pub fn proxies(&'static self) -> &'static Vec<&'static str> {
        &self.proxies
    }

    pub fn checkers_count(&'static self) -> usize {
        self.checkers_count
    }

    pub fn dexscreen_fetch_filters(&'static self) -> &'static Vec<&'static str> {
        &self.dexscreen_fetch_filters
    }

    pub fn dexscreen_fetch_max_pages(&'static self) -> usize {
        self.dexscreen_fetch_max_pages
    }

    pub fn dexscreen_fetch_timeout(&'static self) -> u64 {
        self.dexscreen_fetch_timeout
    }
}

pub fn get_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let config_path = env::var("NOTIFIER_CONFIG").unwrap_or_else(|_| "config.json".to_string());
        let Ok(mut bytes) = fs::read(config_path) else {
            panic!("Failed to read config file");
        };
        let config = simd_json::to_owned_value(&mut bytes).expect("Failed to parse config");
        let mut config = config.into_object().expect("Config should be an object");

        let pumpfun_cookie = Box::leak(
            config
                .remove("pumpfun_cookie")
                .expect("Missing pumpfun_cookie")
                .into_string()
                .expect("pumpfun_cookie should be a string")
                .into_boxed_str(),
        );

        let dexscreen_cookie = Box::leak(
            config
                .remove("dexscreen_cookie")
                .expect("Missing dexscreen_cookie")
                .into_string()
                .expect("dexscreen_cookie should be a string")
                .into_boxed_str(),
        );

        let proxies = config
            .remove("proxies")
            .expect("Missing proxies")
            .into_array()
            .expect("proxies should be an array")
            .into_iter()
            .filter_map(|v| v.into_string())
            .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
            .collect::<Vec<&'static str>>();

        let checkers_count = config
            .remove("checkers_count")
            .expect("Missing checkers_count")
            .as_u64()
            .expect("checkers_count should be a positive number")
            as usize;

        let dexscreen_fetch_filters = config
            .remove("dexscreen_fetch_filters")
            .expect("Missing dexscreen_fetch_filters")
            .into_array()
            .expect("dexscreen_fetch_filters should be an array")
            .into_iter()
            .filter_map(|v| v.into_string())
            .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
            .collect::<Vec<&'static str>>();

        let dexscreen_fetch_max_pages = config
            .remove("dexscreen_fetch_max_pages")
            .expect("Missing dexscreen_fetch_max_pages")
            .as_u64()
            .expect("dexscreen_fetch_max_pages should be a positive number")
            as usize;

        let dexscreen_fetch_timeout = config
            .remove("dexscreen_fetch_timeout")
            .expect("Missing dexscreen_fetch_timeout")
            .as_u64()
            .expect("dexscreen_fetch_timeout should be a positive number");

        let config = Config {
            pumpfun_cookie,
            dexscreen_cookie,
            proxies,
            checkers_count,
            dexscreen_fetch_filters,
            dexscreen_fetch_max_pages,
            dexscreen_fetch_timeout,
        };
        if config.pumpfun_cookie.is_empty() {
            panic!("pumpfun_cookie is empty");
        }
        if config.dexscreen_cookie.is_empty() {
            panic!("dexscreen_cookie is empty");
        }
        if config.proxies.is_empty() {
            panic!("proxies are empty, you need them otherwise we will always reach rate limit");
        }
        config
    })
}
