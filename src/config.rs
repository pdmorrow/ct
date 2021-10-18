use ini::Ini;
use log::{debug, log_enabled, Level::Debug};
use std::collections::HashMap;

#[derive(Debug)]
pub struct StrategyConfig {
    pub members: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ExchangeConfig {
    pub name: String,
    pub uri: String,
    pub version: String,
    pub margin_version: String,
    pub apikey: String,
    pub secretkey: String,
    pub endpoints_map: HashMap<String, String>,
}

#[derive(Debug)]
pub struct Config {
    pub log_level: String,
    pub log_dir: String,
    pub strategy: StrategyConfig,
}

impl Config {
    pub fn get_strategy(&self) -> &StrategyConfig {
        &self.strategy
    }
}

pub fn new(cfg_file_path: &String) -> (Config, ExchangeConfig) {
    let inifile = match Ini::load_from_file("conf/ct.ini") {
        Ok(ini) => ini,

        Err(e) => {
            panic!("failed to load config file {:#?}: {:#?}", cfg_file_path, e);
        }
    };

    if log_enabled!(Debug) {
        debug!("configuration file: ");
        for (section, prop) in inifile.iter() {
            debug!("[{:#?}]", section);
            for (k, v) in prop.iter() {
                debug!("{:#?}={:#?}", k, v);
            }
        }
    }

    let manager_section = match inifile.section(Some("Manager")) {
        Some(s) => s,
        None => panic!("required section \"Manager\" not found!"),
    };

    let exchange_section = match inifile.section(Some("Exchange")) {
        Some(s) => s,
        None => panic!("required section \"Exchange\" not found!"),
    };

    let exchange_name = match exchange_section.get("Name") {
        Some(en) => en,
        None => panic!("section \"Exchange\" missing required \"Name\" entry"),
    };

    let uri = match exchange_section.get("URI") {
        Some(u) => u,
        None => panic!("section \"Exchange\" missing required \"URI\" entry"),
    };

    let version = match exchange_section.get("Version") {
        Some(u) => u,
        None => panic!("section \"Exchange\" missing required \"Version\" entry"),
    };

    let margin_version = match exchange_section.get("MarginVersion") {
        Some(u) => u,
        None => panic!("section \"Exchange\" missing required \"MarginVersion\" entry"),
    };

    let apikey = match exchange_section.get("APIKey") {
        Some(ak) => ak,
        None => panic!("section \"Exchange\" missing required \"APIKey\" entry"),
    };

    let skey = match exchange_section.get("SecretKey") {
        Some(sk) => sk,
        None => panic!("section \"Exchange\" missing required \"SecretKey\" entry"),
    };

    // Read each endpoint entry and add to the hashmap of rest endpoints.
    let eps = match exchange_section.get("Endpoints") {
        Some(eps) => eps,
        None => panic!("section \"Exchange\" missing required \"Endpoints\" entry"),
    };

    // This entry looks like EP0=ep1,EP1=ep1, EP0 is the description of the
    // end point and ep0 is the actual rest end point to add to the api uri.
    let mut endpoints_map: HashMap<String, String> = HashMap::new();
    let endpoints = eps.split(",");
    for ep in endpoints {
        let kv = ep.split("=");
        let kvvec: Vec<&str> = kv.collect();
        endpoints_map.insert(kvvec[0].to_string(), kvvec[1].to_string());
    }

    // Parse [Manager] section, these are global options.
    let log_level = match manager_section.get("LogLevel") {
        Some(v) => v.to_ascii_lowercase(),

        None => "info".to_string(),
    };

    let log_dir = match manager_section.get("LogDir") {
        Some(v) => v.to_ascii_lowercase(),

        None => "info".to_string(),
    };

    // Parse [Strategy] section.
    let strategy_section = match inifile.section(Some("Strategy")) {
        Some(s) => s,
        None => panic!("required section \"Strategy\" not found!"),
    };

    let mut sc = StrategyConfig {
        members: HashMap::with_capacity(strategy_section.len()),
    };
    for (k, v) in strategy_section.iter() {
        sc.members.insert(String::from(k), String::from(v));
    }

    (
        Config {
            strategy: sc,
            log_level: log_level,
            log_dir: log_dir,
        },
        ExchangeConfig {
            name: exchange_name.to_string(),
            uri: uri.to_string(),
            version: version.to_string(),
            margin_version: margin_version.to_string(),
            apikey: apikey.to_string(),
            secretkey: skey.to_string(),
            endpoints_map: endpoints_map,
        },
    )
}
