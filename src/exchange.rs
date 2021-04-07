use crate::config::ExchangeConfig;
use crate::price::Price;
use log::{error};
use std::collections::HashMap;

pub trait Exchange {
    fn new(config: Box<ExchangeConfig>) -> Self where Self: Sized;

    fn get_config(&self) -> &Box<ExchangeConfig>;

    fn get_price(&self, trading_pair: &str) -> Option<Price>;
    
    fn get_prices(&self, trading_pair: Option<Vec<String>>) -> Option<HashMap<String, f64>>;

    fn test_connectivity(&self) -> bool {
        let config = self.get_config();
        let ping_ep = match config.endpoints_map.get(&String::from("PING")) {
            Some(ep) => ep,
            None => {
                panic!("no PING endpoint configured for exchange {:?}", config.name);
            }
        };

        let ping_uri = format!("{}{}", config.uri, ping_ep);
        let client = reqwest::blocking::Client::new();
        match client
            .get(&ping_uri)
            .header("X-MBX-APIKEY", &config.apikey)
            .send()
        {
            Ok(s) => s.status().is_success(),
            Err(e) => {
                error!("connectivity test to {:?} failed: {:?}", config.name, e);
                false
            }
        }
    }
}
