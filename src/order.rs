use crate::binance::{Binance, BinanceErrorCode};
use crate::position;
use crate::tradingpair::TradingPair;

use position::PositionType;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use log::{debug, error, info};

use math::round;

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum OrderType {
    // Simple market order.
    #[allow(dead_code)]
    Market,
    // Limit order.
    #[allow(dead_code)]
    Limit,
}

#[derive(Debug, PartialEq)]
pub enum OrderError {
    Logic,
    NotFilled,
    AccountData,
    NoFunds,
    #[allow(dead_code)]
    MinNotional,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct Fill {
    pub price: String,
    pub qty: String,
    pub commission: String,
    pub commissionAsset: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct OrderResponse {
    symbol: String,
    orderId: i64,
    orderListId: i64,
    clientOrderId: String,
    transactTime: u64,
    price: String,
    origQty: String,
    executedQty: String,
    cummulativeQuoteQty: String,
    status: String,
    timeInForce: String,
    r#type: String,
    side: String,
    pub fills: Vec<Fill>,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct ShortOrderResponse {
    symbol: String,
    orderId: i64,
    clientOrderId: String,
    transactTime: u64,
    pub price: String,
    pub origQty: String,
    pub executedQty: String,
    cummulativeQuoteQty: String,
    pub status: String,
    pub timeInForce: String,
    r#type: String,
    side: String,
    //   marginBuyBorrowAmount: f64,
    //   marginBuyBorrowAsset: String,
    isIsolated: bool,
    pub fills: Vec<Fill>,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct MarginOrderResponse {
    symbol: String,
    orderId: i64,
    clientOrderId: String,
    transactTime: u64,
    price: String,
    origQty: String,
    executedQty: String,
    cummulativeQuoteQty: String,
    status: String,
    timeInForce: String,
    r#type: String,
    side: String,
    marginBuyBorrowAmount: f64,
    marginBuyBorrowAsset: String,
    isIsolated: bool,
    pub fills: Vec<Fill>,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct OrderResponseAck {
    symbol: String,
    orderId: i64,
    orderListId: i64,
    clientOrderId: String,
    transactTime: u64,
}

impl Fill {
    #[allow(dead_code)]
    pub fn get_ave_price(&self) -> f64 {
        self.price.parse::<f64>().unwrap()
    }

    pub fn get_qty(&self) -> f64 {
        self.qty.parse::<f64>().unwrap()
    }

    #[allow(dead_code)]
    pub fn get_commision_paid(&self) -> f64 {
        self.commission.parse::<f64>().unwrap()
    }

    #[allow(dead_code)]
    pub fn get_ave_price_with_commision(&self) -> f64 {
        let qty = self.get_qty();
        ((qty * self.get_ave_price()) + self.get_commision_paid()) / qty
    }
}

pub fn get_average_fill(fills: &Vec<Fill>) -> Option<Fill> {
    if fills.len() > 0 {
        let mut ave_price: f64 = 0.0;
        let mut qty: f64 = 0.0;
        let mut commission: f64 = 0.0;
        for f in fills.iter() {
            ave_price += f.get_ave_price();
            qty += f.get_qty();
            commission += f.get_commision_paid();
        }

        ave_price /= fills.len() as f64;

        Some(Fill {
            price: ave_price.to_string(),
            qty: qty.to_string(),
            commission: commission.to_string(),
            commissionAsset: fills[0].commissionAsset.clone(),
        })
    } else {
        None
    }
}

fn place_order_internal(
    ex: &Binance,
    order_params: &HashMap<&str, &str>,
) -> Result<Fill, OrderError> {
    if let Ok(or) = ex.send_order(order_params, false) {
        if or.status.eq("FILLED") {
            let nfills = or.fills.len();
            let mut price: f64 = 0.0;
            let mut commision: f64 = 0.0;
            let mut qty: f64 = 0.0;
            for f in 0..nfills {
                // TODO: error check the unwraps.
                price += or.fills[f].price.parse::<f64>().unwrap();
                commision += or.fills[f].commission.parse::<f64>().unwrap();
                qty += or.fills[f].qty.parse::<f64>().unwrap();
            }

            return Ok(Fill {
                price: (price / nfills as f64).to_string(),
                qty: qty.to_string(),
                commission: commision.to_string(),
                commissionAsset: or.fills[0].commissionAsset.clone(),
            });
        } else {
            return Err(OrderError::NotFilled);
        }
    } else {
        // TODO: is this teh right error code?
        return Err(OrderError::Logic);
    }
}

fn place_limit_order_internal(
    bex: &Binance,
    tp: &TradingPair,
    position: PositionType,
    qty: f64,
    price: f64,
) -> Result<Fill, OrderError> {
    let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);
    order_params.insert("symbol", tp.symbol());
    order_params.insert("side", "SELL");
    order_params.insert("timeInForce", "IOC");
    order_params.insert("type", "LIMIT");
    let qty_str = qty.to_string();
    order_params.insert("quantity", &qty_str);
    let price_str = price.to_string();
    order_params.insert("price", &price_str);

    if position == PositionType::Long {
        order_params.insert("side", "BUY");
    } else if position == PositionType::Short {
        order_params.insert("side", "SELL");
    }

    let ts_str = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
        .to_string();

    order_params.insert("timestamp", &ts_str);

    place_order_internal(bex, &order_params)
}

fn place_order_quote_quantity(
    ex: &Binance,
    position: PositionType,
    symbol: &str,
    quote_order_quantity: f64,
) -> Result<Fill, OrderError> {
    let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);
    order_params.insert("symbol", symbol);

    if position == PositionType::Long {
        order_params.insert("side", "BUY");
    } else if position == PositionType::Short {
        order_params.insert("side", "SELL");
    } else {
        return Err(OrderError::Logic);
    }

    let q_str = quote_order_quantity.to_string();
    order_params.insert("quoteOrderQty", &q_str);

    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    order_params.insert("timestamp", &t);

    // Market order defaults to FULL.
    order_params.insert("type", "MARKET");
    return place_order_internal(ex, &order_params);
}

pub fn place_order_quantity(
    ex: &Binance,
    position: PositionType,
    tp: &TradingPair,
    quantity: f64,
    limit_price: Option<f64>,
) -> Result<Fill, OrderError> {
    if limit_price.is_some() {
        return place_limit_order_internal(ex, tp, position, quantity, limit_price.unwrap());
    } else {
        let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);
        order_params.insert("symbol", tp.symbol());

        if position == PositionType::Long {
            order_params.insert("side", "BUY");
        } else if position == PositionType::Short {
            order_params.insert("side", "SELL");
        } else {
            return Err(OrderError::Logic);
        }

        // Market order defaults to FULL.
        let q_str = quantity.to_string();
        order_params.insert("quantity", &q_str);
        order_params.insert("type", "MARKET");

        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        order_params.insert("timestamp", &t);

        return place_order_internal(ex, &order_params);
    }
}

pub fn place_stop_limit(
    ex: &Binance,
    tp: &TradingPair,
    quantity: f64,
    stop_trigger_price: f64,
    limit_price: f64,
) -> Result<OrderResponseAck, OrderError> {
    let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);

    order_params.insert("symbol", tp.symbol());
    order_params.insert("side", "SELL");

    let q_str = quantity.to_string();
    order_params.insert("quantity", &q_str);

    order_params.insert("type", "STOP_LOSS_LIMIT");
    order_params.insert("timeInForce", "GTC");

    // Set the trigger price.
    let p_str = stop_trigger_price.to_string();
    order_params.insert("stopPrice", &p_str);

    let p_str = limit_price.to_string();
    order_params.insert("price", &p_str);

    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    order_params.insert("timestamp", &t);

    if let Ok(or) = ex.send_stop_order(&order_params) {
        return Ok(or);
    } else {
        return Err(OrderError::Logic);
    }
}

// Place a stop loss limit order at a price stop_percent percent less than what we just paid.
pub fn place_stop_loss(bex: &Binance, ave_fill: &Fill, tp: &TradingPair, stop_percent: f64) {
    let stop_trigger_price = round::floor(
        ave_fill.get_ave_price() - (stop_percent * tp.get_tick_size()),
        tp.get_price_dps(),
    );

    // Setting the limit price slightly below the trigger price should let the order
    // be filled quickly.
    let stop_limit_price =
        round::floor(stop_trigger_price - tp.get_tick_size(), tp.get_price_dps());

    let mut ncoins = ave_fill.get_qty();
    if ave_fill.commissionAsset.eq(tp.sell_currency()) {
        ncoins -= ave_fill.get_commision_paid();
    }

    ncoins = round::floor(ncoins, tp.get_qty_dps());

    match place_stop_limit(&bex, &tp, ncoins, stop_trigger_price, stop_limit_price) {
        Ok(_) => {
            info!(
                "[STOP-LOSS] stop loss order accepted {:#?} qty:{:.4$} trigger:{:.5$} limit:{:.5$}",
                tp.symbol(),
                ncoins,
                stop_trigger_price,
                stop_limit_price,
                tp.get_qty_dps() as usize,
                tp.get_price_dps() as usize,
            );
        }

        Err(code) => {
            error!("[STOP-LOSS] failed to place, will sell off: {:#?}", code);
            match place_order_quantity(&bex, PositionType::Short, &tp, ncoins, None) {
                Ok(o) => {
                    info!(
                        "[SELL][MARKET] order filled {:#?} qty:{:.3$}, price:{:.4$}",
                        tp.symbol(),
                        o.get_qty(),
                        o.get_ave_price(),
                        tp.get_qty_dps() as usize,
                        tp.get_price_dps() as usize,
                    );
                }
                Err(e) => {
                    error!(
                        "[SELL][MARKET] failed to complete emergency sell off: {:#?}",
                        e
                    );
                }
            }
        }
    }
}

pub fn buy(
    bex: &Binance,            // Exchange handle.
    tp: &TradingPair,         // Trading Pair.
    stop_price: Option<f64>,  // Optional initial stop loss trigger price.
    split_pct: u8,            // Asset split percentage.
    limit_price: Option<f64>, // Optional limit price.
) -> Result<Fill, OrderError> {
    if let Ok(ad) = bex.get_account_data() {
        let qty_dps = tp.get_qty_dps();
        let price_dps = tp.get_price_dps();
        let min_order = tp.get_min_notional();
        let mut spend_assets = round::floor(
            ad.get_balance(tp.buy_currency()).unwrap() * (split_pct / 100) as f64,
            qty_dps,
        );

        let nlocked_assets =
            round::floor(ad.get_locked_balance(tp.buy_currency()).unwrap(), qty_dps);

        if nlocked_assets > 0.0 {
            // We have some balance locked, this means we have a stop that wasn't closed, close it
            // now.
            debug!(
                "[BUY] {:#?} has a locked balance of {:.2$}, close all stops",
                tp.symbol(),
                nlocked_assets,
                qty_dps as usize
            );
            match bex.cancel_all_orders(tp.symbol()) {
                Ok(_) => {
                    debug!("[CANCEL] cancelled all open orders on {:#?}", tp.symbol());
                    spend_assets = nlocked_assets * ((split_pct / 100) as f64);
                }
                Err(code) => {
                    if code != BinanceErrorCode::UnknownOrderSent as i64 {
                        error!("[CANCEL] failed: {:#?}", code);
                    }
                }
            }
        }

        if spend_assets < min_order {
            // Might happen if we initiated trades via the app rather than the bot.
            debug!(
                "[BUY] {:#?} cannot buy, we have {:.3$} to spend but the min order is {:.3$}",
                tp.symbol(),
                spend_assets,
                min_order,
                qty_dps as usize
            );

            return Err(OrderError::NoFunds);
        }

        debug!(
            "[BUY][{:#?}] submit order {:#?} of {:#?} {:#?}",
            if limit_price.is_some() {
                "LIMIT"
            } else {
                "MARKET"
            },
            tp.symbol(),
            spend_assets,
            tp.buy_currency(),
        );

        // Send the order.
        let ave_fill = match limit_price {
            None => place_order_quote_quantity(bex, PositionType::Long, tp.symbol(), spend_assets),
            Some(lp) => {
                let qty = round::floor(spend_assets / lp, qty_dps);

                place_order_quantity(bex, PositionType::Long, &tp, qty, limit_price)
            }
        }?;

        info!(
            "[BUY][{:#?}] order filled {:#?} qty:{:.4$}, price:{:.5$}",
            if limit_price.is_none() {
                "MARKET"
            } else {
                "LIMIT"
            },
            tp.symbol(),
            ave_fill.get_qty(),
            ave_fill.get_ave_price(),
            qty_dps as usize,
            price_dps as usize,
        );

        if let Some(sp) = stop_price {
            let stop_trigger_price = round::floor(sp, price_dps);

            let mut ncoins = ave_fill.get_qty();
            if ave_fill.commissionAsset.eq(tp.sell_currency()) {
                ncoins -= ave_fill.get_commision_paid();
            }

            ncoins = round::floor(ncoins, qty_dps);

            // Set the limit price when we trigger.
            let stop_limit_price =
                round::floor(stop_trigger_price - tp.get_tick_size(), tp.get_price_dps());

            match place_stop_limit(bex, tp, ncoins, stop_trigger_price, stop_limit_price) {
                Ok(_) => {
                    info!(
                        "[STOP-LOSS] stop loss order accepted {:#?} qty:{:.4$} trigger:{:.5$} limit:{:.5$}",
                        tp.symbol(),
                        ncoins,
                        stop_trigger_price,
                        stop_limit_price,
                        qty_dps as usize,
                        price_dps as usize,
                    );
                    return Ok(ave_fill);
                }

                Err(e) => {
                    error!(
                        "[STOP-LOSS] stop loss order not accepted, will sell off: {:#?}",
                        e
                    );

                    // Crap, our stop was not accepted - just market sell what we previously
                    // just bought, we waste commission here though.
                    match place_order_quantity(bex, PositionType::Short, &tp, ncoins, None) {
                        Ok(o) => {
                            info!(
                                "[SELL][MARKET] order filled {:#?} qty:{:.3$}, price:{:.4$}",
                                tp.symbol(),
                                o.get_qty(),
                                o.get_ave_price(),
                                qty_dps as usize,
                                price_dps as usize
                            );
                        }
                        Err(e) => {
                            error!(
                                "[SELL][MARKET] failed to complete emergency sell off: {:#?}",
                                e
                            );
                        }
                    }
                    return Err(e);
                }
            }
        } else {
            Ok(ave_fill)
        }
    } else {
        Err(OrderError::AccountData)
    }
}

pub fn sell(bex: &Binance, tp: &TradingPair, limit_price: Option<f64>) -> Result<Fill, OrderError> {
    if let Ok(ad) = bex.get_account_data() {
        let qty_dps = tp.get_qty_dps();
        let price_dps = tp.get_price_dps();
        let min_order = tp.get_min_qty();
        let mut nsell_assets = round::floor(ad.get_balance(tp.sell_currency()).unwrap(), qty_dps);
        let nlocked_assets =
            round::floor(ad.get_locked_balance(tp.sell_currency()).unwrap(), qty_dps);

        if nlocked_assets > 0.0 {
            // We have some balance locked, this means we have a stop that wasn't closed, close it
            // now.
            debug!(
                "[SELL] {:#?} has a locked balance of {:.2$}, close all stops",
                tp.symbol(),
                nlocked_assets,
                qty_dps as usize
            );
            match bex.cancel_all_orders(tp.symbol()) {
                Ok(_) => {
                    debug!("[CANCEL] cancelled all open orders on {:#?}", tp.symbol());
                    nsell_assets = nlocked_assets;
                }
                Err(code) => {
                    if code != BinanceErrorCode::UnknownOrderSent as i64 {
                        error!("[CANCEL] failed: {:#?}", code);
                    }
                }
            }
        }

        if nsell_assets <= min_order {
            // We may have got stopped out previously of this is just the first
            // time the algorithm is running.
            debug!(
                "[SELL] {:#?} cannot sell, we own {:.3$} but the min order is {:.3$}",
                tp.symbol(),
                nsell_assets,
                min_order,
                qty_dps as usize
            );
            return Err(OrderError::NoFunds);
        }

        let ave_fill =
            place_order_quantity(bex, PositionType::Short, &tp, nsell_assets, limit_price);
        match ave_fill {
            Err(e) => {
                return Err(e);
            }
            Ok(f) => {
                info!(
                    "[SELL][{:#?}] order filled {:#?} qty:{:.4$}, price:{:.5$}",
                    if limit_price.is_none() {
                        "MARKET"
                    } else {
                        "LIMIT"
                    },
                    tp.symbol(),
                    f.get_qty(),
                    f.get_ave_price(),
                    qty_dps as usize,
                    price_dps as usize,
                );

                return Ok(f);
            }
        }
    } else {
        return Err(OrderError::AccountData);
    }
}

// Cancel all open orders, then close any open positions.
pub fn cancel_and_sell_all(bex: &Binance, tp: &TradingPair) {
    match bex.cancel_all_orders(tp.symbol()) {
        Ok(_) => {
            debug!("[CANCEL] cancelled all open orders on {:#?}", tp.symbol());
        }
        Err(code) => {
            if code != BinanceErrorCode::UnknownOrderSent as i64 {
                error!("[CANCEL] failed: {:#?}", code);
            }
        }
    }

    match sell(&bex, &tp, None) {
        Ok(fill) => {
            debug!(
                "[SELL][MARKET] {:#?} complete, qty: {:#?}, price: {:#?}",
                tp.symbol(),
                fill.get_qty(),
                fill.get_ave_price()
            );
        }

        Err(e) => {
            error!("[SELL][MARKET] {:#?} failed: {:#?}", tp.symbol(), e);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::binance;
    use crate::config;
    use crate::tradingpair;
    use crate::utils;

    use log::{error, info};
    use math::round;

    #[test]
    fn market_buy_market_sell() {
        let areyousure = false;
        if areyousure {
            utils::init_logging("testlogs/order/market_buy_market_sell", "debug");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = binance::Binance::new(exchange_config);
            let tp = tradingpair::TradingPair::new(&bex, "ADA/USDT");

            // Try to buy 15 ADA.
            let qty = 15.0;

            let filled_qty = match place_order_quantity(&bex, PositionType::Long, &tp, qty, None) {
                Ok(f) => {
                    info!("[BUY] fill: {:?}", f);
                    f.get_qty()
                }
                Err(e) => {
                    panic!("failed to buy: {:?}", e);
                }
            };

            match place_order_quantity(&bex, PositionType::Short, &tp, filled_qty, None) {
                Ok(f) => {
                    info!("[SELL] fill: {:?}", f);
                }
                Err(e) => {
                    panic!("failed to sell: {:?}", e);
                }
            }
        }
    }

    #[test]
    fn limit_buy_limit_sell() {
        let areyousure = true;
        if areyousure {
            utils::init_logging("testlogs/order/limit_buy_limit_sell", "debug");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = binance::Binance::new(exchange_config);
            let tp = tradingpair::TradingPair::new(&bex, "ADA/USDT");

            // Get last closing price and see if we can place a limit order
            // close to that.
            let mut req_params: HashMap<&str, &str> = HashMap::with_capacity(3);
            req_params.insert("symbol", tp.symbol());
            req_params.insert("interval", "1m");
            req_params.insert("limit", "1");
            if let Ok(cd) = bex.get_cstick_data(&req_params) {
                let cp = cd[0].close_price.parse::<f64>().unwrap();
                let limit_price = Some(cp);
                info!("try to buy @ {:?}", limit_price);
                let _filled_qty = match buy(&bex, &tp, None, 5, limit_price) {
                    Ok(f) => {
                        info!("[BUY] fill: {:?}", f);
                        f.get_qty()
                    }
                    Err(e) => {
                        panic!("failed to buy: {:?}", e);
                    }
                };

                let limit_price = Some(cp);
                info!("try to sell @ {:?}", limit_price);
                match sell(&bex, &tp, limit_price) {
                    Ok(f) => {
                        info!("[SELL] fill: {:?}", f);
                    }
                    Err(e) => {
                        panic!("failed to sell: {:?}", e);
                    }
                }
            } else {
                panic!("failed to get candlestick data");
            }
        }
    }

    #[test]
    fn cancel_stop_15usdt() {
        // WARNING WARNING WARNING.
        // WARNING: This will buy $15 worth of BTC if areyousure is set to true.
        // WARNING WARNING WARNING.

        let areyousure = false;
        if areyousure {
            utils::init_logging("testlogs/order/cancel_stop_15usdt", "info");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = binance::Binance::new(exchange_config);
            let tp = tradingpair::TradingPair::new(&bex, "ADA/USDT");
            let qty_dps = tp.get_qty_dps();

            // Buy $15 worth of BTC.
            let nusdt = 15.12124;
            let nusdt_rounded = round::floor(nusdt, qty_dps);

            match place_order_quote_quantity(&bex, PositionType::Long, tp.symbol(), nusdt_rounded) {
                Ok(avefill) => {
                    info!(
                        "buy {:#?} {:#?} market average fill: {:#?}",
                        nusdt_rounded,
                        tp.symbol(),
                        avefill
                    );

                    let mut failed = false;

                    let price_dps = tp.get_price_dps();

                    // Place a stop order of some amount less than what we paid.
                    let stop_trigger_price =
                        round::floor(avefill.get_ave_price() * 0.95, price_dps);

                    let qty = round::floor(avefill.get_qty(), qty_dps);

                    // Set the limit price when we trigger.
                    let limit_price =
                        round::floor(stop_trigger_price - (tp.get_tick_size() * 2.0), price_dps);

                    match place_stop_limit(&bex, &tp, qty, stop_trigger_price, limit_price) {
                        Ok(or) => {
                            info!("stop placed: {:#?}", or);

                            // Cancel it.
                            match bex.cancel_all_orders(tp.symbol()) {
                                Ok(co) => {
                                    info!("cancelled stop: {:#?}", co);
                                }
                                Err(code) => {
                                    if code != BinanceErrorCode::UnknownOrderSent as i64 {
                                        error!("[CANCEL] failed: {:#?}", code);
                                        failed = true;
                                    }
                                }
                            }
                        }

                        Err(e) => {
                            error!("failed to place stop: {:#?}", e);
                            failed = true;
                        }
                    }

                    // Sell manually.
                    let mut filled_qty = avefill.get_qty();
                    if avefill
                        .commissionAsset
                        .eq_ignore_ascii_case(tp.sell_currency())
                    {
                        filled_qty -= avefill.commission.parse::<f64>().unwrap();
                    }

                    filled_qty = round::floor(filled_qty, qty_dps);

                    match place_order_quantity(&bex, PositionType::Short, &tp, filled_qty, None) {
                        Ok(avefill) => {
                            info!(
                                "sell {:#?} {:#?} market average fill: {:#?}",
                                qty,
                                tp.symbol(),
                                avefill
                            );
                        }

                        Err(e) => {
                            panic!(
                                "sell  {:#?} {:#?} market place_order_quote_quantity failed: {:#?}",
                                qty,
                                tp.symbol(),
                                e
                            );
                        }
                    }

                    if failed {
                        panic!("failed to cancel stop");
                    }
                }
                Err(e) => {
                    panic!(
                        "sell  {:#?} {:#?} market place_order_quote_quantity failed: {:#?}",
                        nusdt,
                        tp.symbol(),
                        e
                    );
                }
            }
        }
    }

    #[test]
    fn buy_sell_market_15usdt() {
        // WARNING WARNING WARNING.
        // WARNING: This will buy $15 worth of BTC if areyousure is set to true.
        // WARNING WARNING WARNING.

        //let areyousure = false;
        let areyousure = false;
        if areyousure {
            utils::init_logging("testlogs/order/buy_sell_market_15usdt", "info");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = binance::Binance::new(exchange_config);
            let tp = tradingpair::TradingPair::new(&bex, "BTC/USDT");
            let qty_dps = tp.get_qty_dps();

            let nusdt = 15.0;

            // BUY.
            match place_order_quote_quantity(&bex, PositionType::Long, tp.symbol(), nusdt) {
                Ok(avefill) => {
                    info!(
                        "buy {:#?} {:#?} market average fill: {:#?}",
                        nusdt,
                        tp.symbol(),
                        avefill
                    );

                    // SELL.
                    let mut filled_qty = avefill.get_qty();
                    info!("filled qty: {:#?}", filled_qty);

                    if avefill
                        .commissionAsset
                        .eq_ignore_ascii_case(tp.sell_currency())
                    {
                        filled_qty -= avefill.commission.parse::<f64>().unwrap();
                    }

                    filled_qty = round::floor(filled_qty, qty_dps);
                    info!("filled qty rounded minus commission: {:#?}", filled_qty);

                    match place_order_quantity(&bex, PositionType::Short, &tp, filled_qty, None) {
                        Ok(sellfill) => {
                            info!(
                                "sell {:#?} {:#?} market average fill: {:#?}",
                                round::floor(sellfill.get_qty(), qty_dps),
                                tp.symbol(),
                                sellfill
                            );
                        }

                        Err(e) => {
                            panic!(
                                "sell  {:#?} {:#?} market place_order_quote_quantity failed: {:#?}",
                                filled_qty,
                                tp.symbol(),
                                e
                            );
                        }
                    }
                }

                Err(e) => {
                    panic!(
                        "sell  {:#?} {:#?} market place_order_quote_quantity failed: {:#?}",
                        nusdt,
                        tp.symbol(),
                        e
                    );
                }
            }
        }
    }
}
