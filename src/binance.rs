// routines for interacting with the Binance REST API.
use crate::account;
use crate::candlestick::CandleStick;
use crate::config::ExchangeConfig;
use crate::exchangeinfo::{LotSizeFilter, PriceFilter};
use crate::order;
use crate::orderbook::OrderBook;
use crate::price::Price;
use crate::utils;

use account::{Account, IsolatedMarginAccount};
use order::{OrderResponseAck, ShortOrderResponse};

use log::error;
use std::collections::HashMap;
use std::str;

use serde::{Deserialize, Serialize};
use serde_json;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum BinanceErrorCode {
    #[allow(dead_code)]
    InsufficientBalance = -2010,
}

#[allow(dead_code)]
pub enum MarginXferDir {
    ToMargin,
    FromMargin,
}

#[derive(Debug)]
pub struct Binance {
    config: ExchangeConfig,
    blocking_client: reqwest::blocking::Client,
}

impl Binance {
    pub fn new(config: ExchangeConfig) -> Self {
        Binance {
            config: config,
            blocking_client: reqwest::blocking::Client::new(),
        }
    }

    pub fn get_config(&self) -> &ExchangeConfig {
        &self.config
    }

    fn get_blocking_client(&self) -> &reqwest::blocking::Client {
        &self.blocking_client
    }

    fn post(
        &self,
        endpoint: &str,
        params: Option<&HashMap<&str, &str>>,
        config: &ExchangeConfig,
        sign: bool,
        margin: bool,
        isolated: bool,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        if isolated {
            assert!(margin);
        }

        let uri = match margin {
            true => match isolated {
                true => {
                    format!(
                        "{}/{}/margin/isolated/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
                false => {
                    format!(
                        "{}/{}/margin/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
            },

            false => {
                format!("{}/{}/{}", config.uri, config.version, endpoint)
            }
        };

        let client = self.get_blocking_client();

        let req = if params.is_some() {
            client
                .post(&uri)
                .header("X-MBX-APIKEY", &config.apikey)
                .query(&params)
        } else {
            client.post(&uri).header("X-MBX-APIKEY", &config.apikey)
        };

        if sign && params.is_some() {
            let hmac = utils::sign_query(&self.config.secretkey, params.unwrap());
            req.query(&[("signature", &hmac)]).send()
        } else {
            req.send()
        }
    }

    fn put(
        &self,
        endpoint: &str,
        params: Option<&HashMap<&str, &str>>,
        config: &ExchangeConfig,
        sign: bool,
        margin: bool,
        isolated: bool,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        if isolated {
            assert!(margin);
        }

        let uri = match margin {
            true => match isolated {
                true => {
                    format!(
                        "{}/{}/margin/isolated/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
                false => {
                    format!(
                        "{}/{}/margin/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
            },

            false => {
                format!("{}/{}/{}", config.uri, config.version, endpoint)
            }
        };

        let client = self.get_blocking_client();

        let req = if params.is_some() {
            client
                .put(&uri)
                .header("X-MBX-APIKEY", &config.apikey)
                .query(&params)
        } else {
            client.put(&uri).header("X-MBX-APIKEY", &config.apikey)
        };

        if sign && params.is_some() {
            let hmac = utils::sign_query(&self.config.secretkey, params.unwrap());
            req.query(&[("signature", &hmac)]).send()
        } else {
            req.send()
        }
    }

    #[allow(dead_code)]
    fn delete(
        &self,
        endpoint: &str,
        params: &HashMap<&str, &str>,
        config: &ExchangeConfig,
        sign: bool,
        margin: bool,
        isolated: bool,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        let uri = match margin {
            true => match isolated {
                true => {
                    format!(
                        "{}/{}/margin/isolated/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
                false => {
                    format!(
                        "{}/{}/margin/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
            },

            false => {
                format!("{}/{}/{}", config.uri, config.version, endpoint)
            }
        };

        let client = self.get_blocking_client();

        let req = client
            .delete(&uri)
            .header("X-MBX-APIKEY", &config.apikey)
            .query(&params);

        if sign {
            let hmac = utils::sign_query(&self.config.secretkey, &params);
            req.query(&[("signature", &hmac)]).send()
        } else {
            req.send()
        }
    }

    fn get(
        &self,
        endpoint: &str,
        params: Option<&HashMap<&str, &str>>,
        config: &ExchangeConfig,
        sign: bool,
        margin: bool,
        isolated: bool,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        let uri = match margin {
            true => match isolated {
                true => {
                    format!(
                        "{}/{}/margin/isolated/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
                false => {
                    format!(
                        "{}/{}/margin/{}",
                        config.uri, config.margin_version, endpoint
                    )
                }
            },

            false => {
                format!("{}/{}/{}", config.uri, config.version, endpoint)
            }
        };

        let client = self.get_blocking_client();

        let req = client.get(&uri).header("X-MBX-APIKEY", &config.apikey);
        if params.is_some() {
            let q = params.unwrap();
            if sign {
                let hmac = utils::sign_query(&self.config.secretkey, &q);
                return req.query(&q).query(&[("signature", &hmac)]).send();
            } else {
                return req.query(&q).send();
            }
        } else {
            return req.send();
        }
    }

    fn get_retries(
        &self,
        endpoint: &str,
        params: Option<&HashMap<&str, &str>>,
        config: &ExchangeConfig,
        sign: bool,
        margin: bool,
        isolated: bool,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        let mut n = 0;
        let tries = 5;
        while n < tries - 1 {
            match self.get(endpoint, params, config, sign, margin, isolated) {
                Ok(r) => {
                    return Ok(r);
                }
                Err(e) => {
                    error!("{:?}", e);
                }
            }

            n += 1;
        }

        return self.get(endpoint, params, config, sign, margin, isolated);
    }

    #[allow(dead_code)]
    pub fn test_connectivity(&self) -> bool {
        let config = self.get_config();
        let ping_ep = match config.endpoints_map.get(&String::from("PING")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no PING endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.get_retries(&ping_ep, None, &config, false, false, false) {
            Ok(s) => {
                return s.status().is_success();
            }

            Err(e) => {
                error!("connectivity test to {:#?} failed: {:#?}", config.name, e);
                false
            }
        }
    }

    /**************************************************************************
     * MARGIN ROUTINES. *******************************************************
     *************************************************************************/
    #[allow(dead_code)]
    pub fn isolated_margin_xfer(
        &self,
        asset: &str,
        isolated_symbol: &str,
        amount: f64,
        direction: MarginXferDir,
    ) -> Result<u64, i64> {
        let config = self.get_config();
        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("asset", asset);
        params.insert("symbol", isolated_symbol);
        let amount_str = amount.to_string();
        params.insert("amount", &amount_str);
        match direction {
            MarginXferDir::ToMargin => {
                params.insert("transFrom", "SPOT");
                params.insert("transTo", "ISOLATED_MARGIN");
            }
            MarginXferDir::FromMargin => {
                params.insert("transTo", "SPOT");
                params.insert("transFrom", "ISOLATED_MARGIN");
            }
        }

        match self.post("transfer", Some(&params), &config, true, true, true) {
            Ok(s) => {
                if s.status().is_success() {
                    let j: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(j["tranId"].as_u64().unwrap());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to account xfer message: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn cross_margin_xfer(
        &self,
        asset: &str,
        amount: f64,
        direction: MarginXferDir,
    ) -> Result<u64, i64> {
        let config = self.get_config();
        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("asset", asset);
        let amount_str = amount.to_string();
        params.insert("amount", &amount_str);
        match direction {
            MarginXferDir::ToMargin => {
                params.insert("type", "1");
            }
            MarginXferDir::FromMargin => {
                params.insert("type", "2");
            }
        }

        match self.post("transfer", Some(&params), &config, true, true, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let j: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(j["tranId"].as_u64().unwrap());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to account xfer message: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn margin_repay(
        &self,
        asset: &str,
        isolated_symbol: Option<&str>,
        amount: f64,
    ) -> Result<u64, i64> {
        let config = self.get_config();
        let repay_ep = match config.endpoints_map.get(&String::from("REPAY")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no REPAY endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };
        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("asset", asset);

        if isolated_symbol.is_some() {
            params.insert("symbol", isolated_symbol.unwrap());
            params.insert("isIsolated", "TRUE");
        }

        let amount_str = amount.to_string();
        params.insert("amount", &amount_str);

        match self.post(repay_ep, Some(&params), &config, true, true, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let j: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(j["tranId"].as_u64().unwrap());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send margin repay message: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn margin_borrow(
        &self,
        asset: &str,
        isolated_symbol: Option<&str>,
        amount: f64,
    ) -> Result<u64, i64> {
        let config = self.get_config();
        let borrow_ep = match config.endpoints_map.get(&String::from("BORROW")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no BORROW endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };
        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("asset", asset);

        if isolated_symbol.is_some() {
            params.insert("symbol", isolated_symbol.unwrap());
            params.insert("isIsolated", "TRUE");
        }

        let amount_str = amount.to_string();
        params.insert("amount", &amount_str);

        match self.post(borrow_ep, Some(&params), &config, true, true, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let j: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(j["tranId"].as_u64().unwrap());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send margin borrow message: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn margin_cancel_all_orders(
        &self,
        symbol: &str,
        isolated: bool,
    ) -> Result<serde_json::Value, i64> {
        let config = self.get_config();
        let co_ep = match config.endpoints_map.get(&String::from("CANCEL_OPEN")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no CANCEL_OPEN endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("symbol", symbol);
        if isolated {
            params.insert("isIsolated", "TRUE");
        }

        match self.delete(&co_ep, &params, &config, true, true, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let v: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(v);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send cancel margin orders: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_isolated_margin_account_data(
        &self,
        symbols: &str,
    ) -> Result<IsolatedMarginAccount, i64> {
        let config = self.get_config();
        let account_ep = match config.endpoints_map.get(&String::from("ACCOUNT_INFO")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ACCOUNT_INFO endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("symbols", symbols);

        match self.get_retries(&account_ep, Some(&params), &config, true, true, true) {
            Ok(s) => {
                if s.status().is_success() {
                    let acc: IsolatedMarginAccount = s.json().unwrap();
                    return Ok(acc);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get isolated margin account data: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn send_short_order(
        &self,
        params: &HashMap<&str, &str>,
    ) -> Result<ShortOrderResponse, i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("ORDER")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ORDER endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.post(&order_ep, Some(&params), &config, true, true, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let or: ShortOrderResponse = s.json().unwrap();
                    return Ok(or);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send order: {:#?}", e);
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn send_margin_order(
        &self,
        params: &HashMap<&str, &str>,
    ) -> Result<ShortOrderResponse, i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("ORDER")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ORDER endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.post(&order_ep, Some(&params), &config, true, true, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let or: ShortOrderResponse = s.json().unwrap();
                    return Ok(or);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send order: {:#?}", e);
                return Err(-1);
            }
        }
    }

    /**************************************************************************
     * SPOT ROUTINES. *********************************************************
     *************************************************************************/
    pub fn create_listen_key(&self) -> Result<String, i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("SPOT_USER_STREAM")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no SPOT_USER_STREAM endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.post(&order_ep, None, &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let text = &s.text().unwrap();
                    let j: serde_json::Value = serde_json::from_str(text).unwrap();
                    let escaped_str = j["listenKey"].to_string();
                    return Ok(serde_json::from_str(&escaped_str).unwrap());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send create listen key request: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn ping_listen_key(&self, listen_key: String) -> Result<(), i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("SPOT_USER_STREAM")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no SPOT_USER_STREAM endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        params.insert("listenKey", &listen_key);

        match self.put(&order_ep, Some(&params), &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    return Ok(());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send refresh listen key request: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn delete_listen_key(&self, listen_key: String) -> Result<(), i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("SPOT_USER_STREAM")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no SPOT_USER_STREAM endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        params.insert("listenKey", &listen_key);

        match self.delete(&order_ep, &params, &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    return Ok(());
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send delete listen key request: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn send_stop_order(&self, params: &HashMap<&str, &str>) -> Result<OrderResponseAck, i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("ORDER")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ORDER endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.post(&order_ep, Some(&params), &config, true, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let or: OrderResponseAck = s.json().unwrap();
                    return Ok(or);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send order: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn send_order(
        &self,
        params: &mut HashMap<&str, &str>,
        margin: bool,
    ) -> Result<OrderResponseAck, i64> {
        let config = self.get_config();
        let order_ep = match config.endpoints_map.get(&String::from("ORDER")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ORDER endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        params.insert("newOrderRespType", "ACK");

        match self.post(&order_ep, Some(&params), &config, true, margin, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let or: OrderResponseAck = s.json().unwrap();
                    return Ok(or);
                }

                let text = &s.text().unwrap();
                error!("failed to send order for {:#?}: {:#?}", params, text);

                // Return the status code from binance.
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send order: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn cancel_all_orders(&self, symbol: &str) -> Result<serde_json::Value, i64> {
        let config = self.get_config();
        let co_ep = match config.endpoints_map.get(&String::from("OPEN_ORDERS")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no OPEN_ORDERS endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("symbol", symbol);

        match self.delete(&co_ep, &params, &config, true, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let v: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(v);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to send cancel order: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn get_open_orders(&self, symbol: &str) -> Result<serde_json::Value, i64> {
        let config = self.get_config();
        let co_ep = match config.endpoints_map.get(&String::from("OPEN_ORDERS")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no OPEN_ORDERS endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);
        params.insert("symbol", symbol);

        match self.get(&co_ep, Some(&params), &config, true, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let v: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(v);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get open orders: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn get_lot_size_filter(&self, symbol: &str) -> Result<LotSizeFilter, i64> {
        match self.get_exchange_info(Some(symbol)) {
            Ok(ei) => {
                let sym = &ei["symbols"][0];
                let lot_size_filter = &sym["filters"][2];
                let step_size = lot_size_filter["stepSize"].as_str().unwrap();
                let decimal_places = utils::decimal_places(&step_size) as i8;

                return Ok(LotSizeFilter {
                    min_qty: lot_size_filter["minQty"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap(),
                    max_qty: lot_size_filter["maxQty"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap(),
                    step_size: step_size.parse::<f64>().unwrap(),
                    decimal_places: decimal_places,
                });
            }

            Err(code) => {
                return Err(code);
            }
        }
    }

    pub fn get_min_notional_filter(&self, symbol: &str) -> Result<f64, i64> {
        match self.get_exchange_info(Some(symbol)) {
            Ok(ei) => {
                let sym = &ei["symbols"][0];
                let min_notional_filter = &sym["filters"][3];
                let min_notional = min_notional_filter["minNotional"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();

                return Ok(min_notional);
            }

            Err(code) => {
                return Err(code);
            }
        }
    }

    pub fn get_price_filter(&self, symbol: &str) -> Result<PriceFilter, i64> {
        match self.get_exchange_info(Some(symbol)) {
            Ok(ei) => {
                let sym = &ei["symbols"][0];
                let price_filter = &sym["filters"][0];
                let tick_size = price_filter["tickSize"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();
                let tick_size_str = tick_size.to_string();
                let whole_and_decimal: Vec<&str> = tick_size_str.split(".").collect();

                return Ok(PriceFilter {
                    max_price: price_filter["maxPrice"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap(),
                    min_price: price_filter["minPrice"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap(),
                    tick_size: tick_size,
                    decimal_places: whole_and_decimal[1].len() as i8,
                });
            }

            Err(code) => {
                return Err(code);
            }
        }
    }

    fn get_exchange_info(&self, symbol: Option<&str>) -> Result<serde_json::Value, i64> {
        let config = self.get_config();
        let ei_ep = match config.endpoints_map.get(&String::from("EXCHANGE_INFO")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no EXCHANGE_INFO endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        params.insert("symbol", symbol.unwrap());

        match self.get_retries(&ei_ep, Some(&params), &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let v: serde_json::Value = serde_json::from_str(&s.text().unwrap()).unwrap();
                    return Ok(v);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get exchange info: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn get_account_data(&self) -> Result<Account, i64> {
        let config = self.get_config();
        let account_ep = match config.endpoints_map.get(&String::from("ACCOUNT_INFO")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ACCOUNT_INFO endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        params.insert("timestamp", &t);

        match self.get_retries(&account_ep, Some(&params), &config, true, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let acc: Account = s.json().unwrap();
                    return Ok(acc);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get account data: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn get_cstick_data(&self, params: &HashMap<&str, &str>) -> Result<Vec<CandleStick>, i64> {
        let config = self.get_config();
        let cstick_ep = match config.endpoints_map.get(&String::from("CSTICK")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no CSTICK endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.get_retries(&cstick_ep, Some(&params), &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let c: Vec<CandleStick> = s.json().unwrap();
                    return Ok(c);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!(
                    "failed to get candle stick data for {:#?}: {:#?}",
                    params, e
                );
                return Err(-1);
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_order_book(&self, symbol: &str, limit: Option<u16>) -> Result<OrderBook, i64> {
        let config = self.get_config();
        let ob_ep = match config.endpoints_map.get(&String::from("ORDER_BOOK")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no ORDER_BOOK endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::new();
        params.insert("symbol", symbol);

        // 100 is the binance default.
        let l = limit.unwrap_or(100).to_string();
        params.insert("limit", &l);

        match self.get_retries(&ob_ep, Some(&params), &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let ob: OrderBook = s.json().unwrap();
                    return Ok(ob);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get order book: {:#?}", e);
                return Err(-1);
            }
        }
    }

    // Get UNIX epoch ts the server is using.
    pub fn get_server_time(&self) -> Result<u64, i64> {
        let config = self.get_config();
        let st_ep = match config.endpoints_map.get(&String::from("TIME")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no TIME endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        match self.get_retries(&st_ep, None, &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    #[derive(Serialize, Deserialize, Debug)]
                    #[allow(non_snake_case)]
                    struct ST {
                        serverTime: u64,
                    }

                    let time: ST = s.json().unwrap();
                    return Ok(time.serverTime);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get server time: {:#?}", e);
                return Err(-1);
            }
        }
    }

    pub fn get_price(&self, trading_pair: &str) -> Result<Price, i64> {
        let config = self.get_config();
        let price_ep = match config.endpoints_map.get(&String::from("PRICE")) {
            Some(ep) => ep,
            None => {
                panic!(
                    "no PRICE endpoint configured for exchange {:#?}",
                    config.name
                );
            }
        };

        let mut params: HashMap<&str, &str> = HashMap::with_capacity(1);
        params.insert("symbol", trading_pair);

        match self.get_retries(&price_ep, Some(&params), &config, false, false, false) {
            Ok(s) => {
                if s.status().is_success() {
                    let p: Price = s.json().unwrap();
                    // TODO: check we could deserialize.
                    return Ok(p);
                }

                // Return the status code from binance.
                let text = &s.text().unwrap();
                let j: serde_json::Value = serde_json::from_str(text).unwrap();
                error!("{}", text);
                return Err(j["code"].as_i64().unwrap());
            }

            Err(e) => {
                error!("failed to get price for {:#?}: {:#?}", trading_pair, e);
                return Err(-1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config;

    use crate::tradingpair::TradingPair;

    use log::info;

    #[test]
    fn get_price() {
        utils::init_logging("testlogs/binance/get_price", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "ADA/USDT");

        // Price of BTCUPUSDT.
        match bex.get_price(tp.symbol()) {
            Ok(p) => {
                info!("price: {:#?}", p);
            }
            Err(code) => {
                panic!("failed to get price data for {:#?}: {:#?}", tp, code);
            }
        }
    }

    #[test]
    fn get_order_book() {
        utils::init_logging("testlogs/binance/get_order_book", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "ADA/USDT");

        // Order book of ADAUSDT.
        match bex.get_order_book(tp.symbol(), None) {
            Ok(ob) => {
                info!("{:#?}", ob);
            }
            Err(code) => {
                panic!("failed to get order book for {:#?}: {:#?}", tp, code);
            }
        }
    }

    #[test]
    fn get_exchange_info() {
        utils::init_logging("testlogs/binance/get_exchange_info", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "BTC/USDT");

        // Trading information about BTCUPUSDT.
        match bex.get_exchange_info(Some(tp.symbol())) {
            Ok(ei) => {
                info!("{:#?}", ei);
            }
            Err(code) => {
                panic!(
                    "failed to get exchange info data for {:#?}: {:#?}",
                    tp, code
                );
            }
        }
    }

    #[test]
    fn get_price_filter() {
        utils::init_logging("testlogs/binance/get_price_filter", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "BTCUP/USDT");

        // Trading information about BTCUSDT.
        match bex.get_price_filter(tp.symbol()) {
            Ok(pf) => {
                info!("{:?}", pf);
            }
            Err(code) => {
                panic!("failed to get price filter data for {:#?}: {:#?}", tp, code);
            }
        }
    }

    #[test]
    fn get_min_notional() {
        utils::init_logging("testlogs/binance/get_min_notional", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "ADA/USDT");

        match bex.get_min_notional_filter(tp.symbol()) {
            Ok(mn) => {
                info!("{:?}", mn);
            }
            Err(code) => {
                panic!("failed to get min notional data for {:#?}: {:#?}", tp, code);
            }
        }
    }

    #[test]
    fn get_lot_size_filter() {
        utils::init_logging("testlogs/binance/get_lot_size_filter", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "BTC/USDT");

        // Lot information about BTCUSDT.
        match bex.get_lot_size_filter(tp.symbol()) {
            Ok(pf) => {
                info!("{:?}", pf);
            }
            Err(code) => {
                panic!(
                    "failed to get lot size filter data for {:?}: {:?}",
                    tp, code
                );
            }
        }
    }

    #[test]
    fn connection_test() {
        utils::init_logging("testlogs/binance/connection_test", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let conntest = bex.test_connectivity();
        assert!(conntest == true);
    }

    #[test]
    fn get_account_data() {
        utils::init_logging("testlogs/binance/get_account_data", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let ad = bex.get_account_data();
        assert!(ad.is_ok());
        info!("{:#?}", ad.unwrap());
    }

    #[test]
    fn get_isolated_margin_account_data() {
        utils::init_logging("testlogs/binance/get_isolated_margin_account_data", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let ad = bex.get_isolated_margin_account_data("ADAUSDT");
        assert!(ad.is_ok());
        info!("{:#?}", ad.unwrap());
    }

    #[test]
    fn cross_margin_account_xfer() {
        utils::init_logging("testlogs/binance/cross_margin_account_xfer", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let trans_id = bex.cross_margin_xfer("USDT", 10.0, MarginXferDir::ToMargin);
        assert!(trans_id.is_ok());
        let trans_id = bex.cross_margin_xfer("USDT", 10.0, MarginXferDir::FromMargin);
        assert!(trans_id.is_ok());
    }

    #[test]
    fn isolated_margin_account_xfer() {
        utils::init_logging("testlogs/binance/isolated_margin_account_xfer", "info");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let trans_id = bex.isolated_margin_xfer("USDT", "ADAUSDT", 10.0, MarginXferDir::ToMargin);
        assert!(trans_id.is_ok());
        let trans_id = bex.isolated_margin_xfer("USDT", "ADAUSDT", 10.0, MarginXferDir::FromMargin);
        assert!(trans_id.is_ok());
    }
}
