use crate::strategy;
use ini::Ini;
use log::{debug, log_enabled, Level::Debug};
use std::collections::HashMap;

#[derive(Debug)]
pub struct ExchangeConfig {
    pub name: String,
    pub uri: String,
    pub apikey: String,
    pub endpoints_map: HashMap<String, String>,
}

#[derive(Debug)]
pub struct Config {
    pub exchange: ExchangeConfig,
    pub dryrun: bool,
    pub strategies: strategy::StrategyTypes,
}

pub fn new(cfg_file_path: &String) -> Config {
    let inifile = match Ini::load_from_file("conf/ct.ini") {
        Ok(ini) => ini,

        Err(e) => {
            panic!("failed to load config file {:?}: {:?}", cfg_file_path, e);
        }
    };

    if log_enabled!(Debug) {
        debug!("configuration file: ");
        for (section, prop) in inifile.iter() {
            debug!("[{:?}]", section);
            for (k, v) in prop.iter() {
                debug!("{:?}={:?}", k, v);
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

    let strat_section = match inifile.section(Some("Strategies")) {
        Some(s) => s,
        None => panic!("required section \"Strategies\" not found!"),
    };

    let exchange_name = match exchange_section.get("Name") {
        Some(en) => en,
        None => panic!("section \"Exchange\" missing required \"Name\" entry"),
    };

    let uri = match exchange_section.get("URI") {
        Some(u) => u,
        None => panic!("section \"Exchange\" missing required \"URI\" entry"),
    };

    let apikey = match exchange_section.get("APIKey") {
        Some(ak) => ak,
        None => panic!("section \"Exchange\" missing required \"APIKey\" entry"),
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
    //
    // dryrun indicates whether we actually trade or not.
    let dryrun = match manager_section.get("Dryrun") {
        Some(v) => {
            if v.eq_ignore_ascii_case("true") {
                true
            } else if v.eq_ignore_ascii_case("yes") {
                true
            } else {
                false
            }
        }

        None => false,
    };

    // Parse [Strategies] section.
    let enabled_strats = match strat_section.get("Enabled") {
        Some(es) => es,
        None => panic!("section \"Strategies\" missing required \"Enabled\" entry"),
    };

    // Enabled is a comma separated list of enabled strategies.
    let strat_bitmask = strategy::from_str_cs(enabled_strats);

    Config {
        exchange: ExchangeConfig {
            name: exchange_name.to_string(),
            uri: uri.to_string(),
            apikey: apikey.to_string(),
            endpoints_map: endpoints_map,
        },
        
        strategies: strat_bitmask,
        dryrun: dryrun,
    }
}
