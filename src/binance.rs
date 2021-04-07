use crate::config::ExchangeConfig;
use crate::exchange::Exchange;

use std::collections::HashMap;
use crate::price::Price;
use log::{error, info};

#[derive(Debug)]
pub struct Binance {
    pub config: Box<ExchangeConfig>,
}

impl Exchange for Binance {
    fn new(config: Box<ExchangeConfig>) -> Self {
        Binance { config: config }
    }

    fn get_config(&self) -> &Box<ExchangeConfig> {
        &self.config
    }

    fn get_price(&self, trading_pair: &str) -> Option<Price> {
        info!("start get_price");
        let config = self.get_config();
        let price_ep = match config.endpoints_map.get(&String::from("PRICE")) {
            Some(ep) => ep,
            None => {
                panic!("no PRICE endpoint configured for exchange {:?}", config.name);
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::with_capacity(1);
        params.insert("symbol", trading_pair);

        let price_uri = format!("{}{}", config.uri, price_ep);
        let client = reqwest::blocking::Client::new();
        match client.get(&price_uri)
            .header("X-MBX-APIKEY", &config.apikey)
            .query(&params)
            .send()
        {
            Ok(s) => {
                if s.status().is_success() {
                    let p: Price = s.json().unwrap();
                    // TODO: check we could deserialize.
                    info!("end get_price");
                    return Some(p)
                }

                None
            },

            Err(e) => {
                error!("failed to get price for {:?}: {:?}", trading_pair, e);
                None
            }
        }
    }

    fn get_prices(&self, trading_pair: Option<Vec<String>>) -> Option<HashMap<String, f64>> {
        None
    }
}
