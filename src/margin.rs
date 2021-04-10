// Support for managing a margin account as well as trading on margin.
//
// Currently only supports isolated margin.

use crate::binance;
use crate::order;
use crate::position;
use crate::process_md;
use crate::tradingpair;

use binance::Binance;
use position::PositionType;
use tradingpair::TradingPair;

use log::{debug, error, info};

use math::round;

use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::HashMap;

fn short_sell(
    bex: &Binance,
    tp: &TradingPair,
    qty: f64,
    price: Option<f64>,
) -> Result<order::ShortOrderResponse, i64> {
    let mut params: HashMap<&str, &str> = HashMap::new();
    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    params.insert("timestamp", &t);
    params.insert("symbol", tp.symbol());
    params.insert("isIsolated", "TRUE");
    params.insert("side", "SELL");
    params.insert("sideEffectType", "MARGIN_BUY");
    let qty_str = qty.to_string();
    params.insert("quantity", &qty_str);

    if price.is_none() {
        params.insert("type", "MARKET");
        bex.send_short_order(&params)
    } else {
        params.insert("type", "LIMIT");
        params.insert("timeInForce", "GTC");
        let price_str = price.unwrap().to_string();
        params.insert("price", &price_str);
        bex.send_short_order(&params)
    }
}

// Buy back a number coins in order to repair outstanding debt, do this
// using MARKET orders so we gaurantee we don't hold the debt for too long
// (or in error).
//
// Buying with AUTO_REPAY doesn't seem to work, instead just buy with
// no side effect and use the repay API.
fn close_short_position(
    bex: &Binance,
    purchase_qty: f64,
    owed: f64,
    tp: &TradingPair,
) -> Result<order::ShortOrderResponse, i64> {
    let mut params: HashMap<&str, &str> = HashMap::new();
    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    params.insert("timestamp", &t);
    params.insert("symbol", tp.symbol());
    params.insert("isIsolated", "TRUE");
    params.insert("side", "BUY");
    params.insert("type", "MARKET");
    let qty_str = purchase_qty.to_string();
    params.insert("quantity", &qty_str);
    match bex.send_margin_order(&params) {
        Ok(or) => {
            match bex.margin_repay(tp.sell_currency(), Some(tp.symbol()), owed) {
                Ok(_) => {
                    return Ok(or);
                }

                Err(code) => {
                    // This is a problem.
                    return Err(code);
                }
            }
        }
        Err(code) => {
            // This is a problem.
            return Err(code);
        }
    }
}

// Sell "net_assets" number of the trading pair sell currency whilst
// also repaying any debt outstanding on this isolated pair.
fn close_long_position(
    bex: &Binance,
    sell_qty: f64,
    price: Option<f64>,
    owed: Option<f64>,
    tp: &TradingPair,
) -> Result<order::ShortOrderResponse, i64> {
    let mut params: HashMap<&str, &str> = HashMap::new();
    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    params.insert("timestamp", &t);
    params.insert("symbol", tp.symbol());
    params.insert("isIsolated", "TRUE");
    params.insert("side", "SELL");
    let qty_str = sell_qty.to_string();
    params.insert("quantity", &qty_str);

    let or = if price.is_none() {
        params.insert("type", "MARKET");
        bex.send_margin_order(&params)
    } else {
        params.insert("type", "LIMIT");
        // Must be Fill Or Kill if we want to repay the debt immediately.
        params.insert("timeInForce", "FOK");
        let price_str = price.unwrap().to_string();
        params.insert("price", &price_str);
        bex.send_margin_order(&params)
    };

    match or {
        Ok(or) => {
            if owed.is_some() && or.status.eq("FILLED") {
                match bex.margin_repay(tp.buy_currency(), Some(tp.symbol()), owed.unwrap()) {
                    Ok(_) => {
                        return Ok(or);
                    }

                    Err(code) => {
                        // This is a problem.
                        return Err(code);
                    }
                }
            }

            error!("couldn't immediately fill sell order, could not repay debt");

            return Err(-1);
        }
        Err(code) => {
            // This is a problem.
            return Err(code);
        }
    }
}

// Sell "net_assets" number of the trading pair sell currency whilst
// also repaying any debt outstanding on this isolated pair.
fn enter_long_position(
    bex: &Binance,
    spend: f64,
    price: Option<f64>,
    borrow: bool,
    tp: &TradingPair,
) -> Result<order::ShortOrderResponse, i64> {
    let mut params: HashMap<&str, &str> = HashMap::new();
    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    params.insert("timestamp", &t);
    params.insert("symbol", tp.symbol());
    params.insert("isIsolated", "TRUE");
    params.insert("side", "BUY");
    if borrow {
        params.insert("sideEffectType", "MARGIN_BUY");
    }

    let spend_str = spend.to_string();
    if price.is_none() {
        params.insert("type", "MARKET");
        params.insert("quoteOrderQty", &spend_str);

        bex.send_margin_order(&params)
    } else {
        let qty = round::floor(spend / price.unwrap(), tp.get_qty_dps());
        params.insert("type", "LIMIT");
        params.insert("timeInForce", "GTC");
        let qty_str = qty.to_string();
        params.insert("quantity", &qty_str);
        let price_str = price.unwrap().to_string();
        params.insert("price", &price_str);

        bex.send_margin_order(&params)
    }
}

// Place a stop loss limit order at a price stop_percent percent less than what we just paid.
// TODO, need to switch enabling monitoring via websockets, since we currently don't repay debt
// if we hit a stop.
fn place_stop_loss(bex: &Binance, ave_fill: &order::Fill, tp: &TradingPair, stop_percent: f64) {
    let mut params: HashMap<&str, &str> = HashMap::new();
    let ts_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let t = ts_now.to_string();
    params.insert("timestamp", &t);
    params.insert("symbol", tp.symbol());
    params.insert("isIsolated", "TRUE");
    params.insert("side", "SELL");

    let mut ncoins = ave_fill.get_qty();
    if ave_fill.commissionAsset.eq(tp.sell_currency()) {
        ncoins -= ave_fill.get_commision_paid();
    }
    ncoins = round::floor(ncoins, tp.get_qty_dps());

    let qty_str = ncoins.to_string();
    params.insert("quantity", &qty_str);
    params.insert("type", "STOP_LOSS_LIMIT");

    let stop_trigger_price = round::floor(
        ave_fill.get_ave_price() - (stop_percent * tp.get_tick_size()),
        tp.get_price_dps(),
    );
    let stop_limit_price =
        round::floor(stop_trigger_price - tp.get_tick_size(), tp.get_price_dps());

    let stop_limit_price = stop_limit_price.to_string();
    params.insert("price", &stop_limit_price);
    let stop_trigger_price = stop_trigger_price.to_string();
    params.insert("stopPrice", &stop_trigger_price);
    params.insert("timeInForce", "GTC");
    match bex.send_margin_order(&params) {
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
            error!("[STOP-LOSS] failed to place: {:#?}", code);
        }
    };
}

// Margin trade, go long or go short. Repay debts.
pub fn trade(
    bex: &Binance,
    desired_position: PositionType,
    tp: &TradingPair,
    signal_msg: &process_md::TradeThreadMsg,
    leverage: Option<f64>,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
    stop_percent: f64,
) {
    // 1) Cancel any open orders on the pair (i.e. cancel any stops).
    match bex.margin_cancel_all_orders(tp.symbol(), true) {
        Ok(_) => {
            debug!(
                "[CANCEL][MARGIN] cancelled all open orders on {:#?}",
                tp.symbol()
            );
        }
        Err(_) => {}
    }

    let current_price = match bex.get_price(Some(tp.symbol())) {
        Ok(vp) => vp.get(0).unwrap().price.parse::<f64>().unwrap(),
        Err(code) => {
            error!(
                "[BUY][MARGIN] {:?} failed to get price data: {:?}",
                tp.symbol(),
                code
            );
            return;
        }
    };

    info!(
        "[MARGIN] {:?} current price: {:.3$}, close price: {:.3$}",
        tp.symbol(),
        current_price,
        signal_msg.closing_price,
        tp.get_price_dps() as usize,
    );

    if desired_position == PositionType::Long {
        debug!(
            "[BUY][MARGIN] {:#?}: trade thread, got signal to buy",
            tp.symbol()
        );

        // 2) Close short positions.
        match bex.get_isolated_margin_account_data(tp.symbol()) {
            Ok(ad) => {
                let borrowed = &ad.assets[0].baseAsset["borrowed"];
                let borrowed = borrowed.as_str().unwrap().parse::<f64>().unwrap();

                let interest = &ad.assets[0].baseAsset["interest"];
                let interest = interest.as_str().unwrap().parse::<f64>().unwrap();

                // Maker & take fees when not using BNB are 0.1%.
                let owed = interest + borrowed;
                let commision = owed / 1000.0;

                // This the amount we need to buy back in order to repay the initial
                // loan along with interest & commission.
                let purchase_qty = round::ceil(owed + commision, tp.get_qty_dps());

                if current_price * purchase_qty >= tp.get_min_notional() {
                    match close_short_position(bex, purchase_qty, owed, tp) {
                        Ok(or) => {
                            match order::get_average_fill(&or.fills) {
                                Some(ave_fill) => {
                                    info!(
                                        "[BUY][MARGIN] {:?} closed short position {:?} @ {:.3$}",
                                        tp.symbol(),
                                        ave_fill.get_qty(),
                                        ave_fill.get_ave_price(),
                                        tp.get_price_dps() as usize,
                                    );
                                }
                                None => {
                                    // Partially filled or new order state, shouldn't happen today
                                    // as the above is executed as a market order.
                                    info!(
                                        "[BUY][MARGIN] {:?} close short order accepted, state: {:?}({:?}) qty: {:?} filled qty: {:?} @ {:?}",
                                        tp.symbol(),
                                        or.status,
                                        or.timeInForce,
                                        or.origQty,
                                        or.executedQty,
                                        or.price,
                                    );
                                }
                            }
                        }
                        Err(code) => {
                            error!(
								"[BUY][MARGIN] {:?} failed to closed short position borrowed: {:?} interest: {:?}, commission: {:?}: {:?}",
								tp.symbol(),
								borrowed,
								interest,
								commision,
								code
							);
                            return;
                        }
                    }
                } else {
                    if owed > 0.0 {
                        error!(
                            "[BUY][MARGIN] {:?} still owe {:?} {:?}",
                            tp.symbol(),
                            owed,
                            tp.sell_currency()
                        );
                    }
                }
            }

            Err(code) => {
                error!(
                    "[BUY][MARGIN] {:?} failed to get account data: {:?}",
                    tp.symbol(),
                    code
                );
                return;
            }
        }

        // 3) How much do we have to spend?
        let ad = match bex.get_isolated_margin_account_data(tp.symbol()) {
            Ok(ad) => ad,
            Err(code) => {
                error!(
                    "[BUY][MARGIN] {:?} failed to get account data: {:?}",
                    tp.symbol(),
                    code
                );
                return;
            }
        };

        let quote_asset = &ad.assets[0].quoteAsset;
        let avail_quote_asset = quote_asset["free"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let avail_spend = round::floor(avail_quote_asset, tp.get_price_dps());

        // Leverage up if requested.
        let final_spend = match leverage {
            Some(l) => {
                let leveraged_spend = round::floor(avail_spend * l as f64, tp.get_price_dps());
                leveraged_spend
            }

            None => avail_spend,
        };

        let limit_price = match order_type {
            order::OrderType::Market => None,
            order::OrderType::Limit => {
                assert!(limit_offset.is_some());
                Some(signal_msg.closing_price + (limit_offset.unwrap() as f64 * tp.get_tick_size()))
            }
        };

        // 4) Enter the position.
        match enter_long_position(bex, final_spend, limit_price, leverage.is_some(), tp) {
            Ok(or) => {
                match order::get_average_fill(&or.fills) {
                    Some(ave_fill) => {
                        info!(
                            "[BUY][MARGIN] {:?} entered long position {:.3$} @ {:.4$}",
                            tp.symbol(),
                            ave_fill.get_qty(),
                            ave_fill.get_ave_price(),
                            tp.get_qty_dps() as usize,
                            tp.get_price_dps() as usize,
                        );

                        place_stop_loss(&bex, &ave_fill, &tp, stop_percent);
                    }

                    None => {
                        // Partially filled or new order state, shouldn't happen today
                        // as the above is executed as a market order.
                        info!(
                            "[BUY][MARGIN] {:?} long order accepted, state: {:?}({:?}) qty: {:?} filled qty: {:?} @ {:?}",
                            tp.symbol(),
                            or.status,
                            or.timeInForce,
                            or.origQty,
                            or.executedQty,
                            or.price,
                        );
                    }
                }
            }
            Err(code) => {
                error!(
					"[BUY][MARGIN] {:?} failed to enter long position, purchase funds: {:?} {:?}: {:?}",
					tp.symbol(),
					final_spend,
					tp.buy_currency(),
					code,
				);
                return;
            }
        }

        // 5) Set stop loss.
    } else if desired_position == PositionType::Short {
        debug!(
            "[SELL][MARGIN] {:#?}: trade thread, got signal to sell",
            tp.symbol()
        );

        // 2) Close any long positions, i.e. sell what we currently own.
        match bex.get_isolated_margin_account_data(tp.symbol()) {
            Ok(ad) => {
                let free = &ad.assets[0].baseAsset["free"];
                let free = free.as_str().unwrap().parse::<f64>().unwrap();

                let owed = if leverage.is_some() {
                    let borrowed = &ad.assets[0].baseAsset["borrowed"];
                    let borrowed = borrowed.as_str().unwrap().parse::<f64>().unwrap();

                    let interest = &ad.assets[0].baseAsset["interest"];
                    let interest = interest.as_str().unwrap().parse::<f64>().unwrap();

                    if interest + borrowed > 0.0 {
                        Some(interest + borrowed)
                    } else {
                        None
                    }
                } else {
                    None
                };

                let limit_price = match order_type {
                    order::OrderType::Market => None,
                    order::OrderType::Limit => {
                        assert!(limit_offset.is_some());
                        Some(
                            signal_msg.closing_price
                                - (limit_offset.unwrap() as f64 * tp.get_tick_size()),
                        )
                    }
                };

                // Sell everything we have available to sell for this isolated pair.
                let sell_qty = round::floor(free, tp.get_qty_dps());
                if current_price * sell_qty >= tp.get_min_notional() {
                    match close_long_position(bex, sell_qty, limit_price, owed, tp) {
                        Ok(or) => {
                            match order::get_average_fill(&or.fills) {
                                Some(ave_fill) => {
                                    info!(
                                        "[SELL][MARGIN] {:?} closed long position {:.4$} {:?} @ {:?}",
                                        tp.symbol(),
                                        ave_fill.get_qty(),
                                        tp.sell_currency(),
                                        ave_fill.get_ave_price(),
                                        tp.get_qty_dps() as usize,
                                    );
                                }

                                None => {
                                    // Partially filled or new order state, shouldn't happen today
                                    // as the above is executed as a market order.
                                    info!(
                                        "[SELL][MARGIN] {:?} long order close accepted, state: {:?}({:?}) qty: {:?} filled qty: {:?} @ {:?}",
                                        tp.symbol(),
                                        or.status,
                                        or.timeInForce,
                                        or.origQty,
                                        or.executedQty,
                                        or.price,
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            if e != binance::BinanceErrorCode::InsufficientBalance as i64 {
                                error!(
                                    "[SELL][MARGIN] {:?} failed to close long position {:?}: {:?}",
                                    tp.symbol(),
                                    tp.sell_currency(),
                                    e
                                );
                                return;
                            }
                        }
                    }
                } else {
                    if let Some(owed) = owed {
                        if owed > 0.0 {
                            error!(
                                "[SELL][MARGIN] {:?} still owe {:?} {:?}",
                                tp.symbol(),
                                owed,
                                tp.sell_currency()
                            );
                        }
                    }
                }
            }

            Err(code) => {
                error!(
                    "[SELL][MARGIN] {:?} failed to get account data: {:?}",
                    tp.symbol(),
                    code
                );
                return;
            }
        }

        // 3) We always need to borrow when shorting, how much we borrow depends
        //    on how much collateral we have and how much leverage we want to use.
        let ad = bex.get_isolated_margin_account_data(tp.symbol()).unwrap();
        let quote_asset = &ad.assets[0].quoteAsset;
        let net_quote_asset = quote_asset["netAsset"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let base_asset_price = ad.assets[0].indexPrice.parse::<f64>().unwrap();
        let borrow_qty = round::floor(
            (net_quote_asset / base_asset_price)
                * if leverage.is_none() {
                    1.0
                } else {
                    leverage.unwrap() as f64
                },
            tp.get_qty_dps(),
        );

        let limit_price = match order_type {
            order::OrderType::Market => None,
            order::OrderType::Limit => {
                assert!(limit_offset.is_some());
                Some(signal_msg.closing_price - (limit_offset.unwrap() as f64 * tp.get_tick_size()))
            }
        };

        // Borrow and sell in one swoop.
        if current_price * borrow_qty >= tp.get_min_notional() {
            match short_sell(bex, tp, borrow_qty, limit_price) {
                Ok(or) => {
                    let ave_fill = order::get_average_fill(&or.fills);

                    match ave_fill {
                        Some(av) => {
                            info!(
                                "[SELL][MARGIN] {:?} entered short position {:?} {:?} @ {:?} (requested limit: {:?})",
                                tp.symbol(),
                                tp.sell_currency(),
                                av.get_qty(),
                                av.get_ave_price(),
                                limit_price,
                            );

                            place_stop_loss(&bex, &av, &tp, stop_percent * -1.0);
                        }
                        None => {
                            // Partially filled or new order state.
                            info!(
                                "[SELL][MARGIN] {:?} short order accepted {:?}, state: {:?}({:?}) qty: {:?} filled qty: {:?} @ {:?} (requested limit: {:?})",
                                tp.symbol(),
                                tp.sell_currency(),
                                or.status,
                                or.timeInForce,
                                or.origQty,
                                or.executedQty,
                                or.price,
                                limit_price,
                            );
                        }
                    }
                }

                Err(code) => {
                    error!(
                        "[SELL][MARGIN] {:?} failed to enter short position {:?} {:?}: {:?}",
                        tp.symbol(),
                        borrow_qty,
                        tp.sell_currency(),
                        code
                    );
                }
            }
        } else {
            info!(
                "[SELL][MARGIN] {:?} not enough collateral to borrow {:?}",
                tp.symbol(),
                borrow_qty
            );
        }

    // Place stop loss, i.e. buy auto repay.
    } else {
        assert!(desired_position != PositionType::None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config;
    use crate::utils;

    #[test]
    fn _short_sell() {
        let are_you_sure = true;

        if are_you_sure {
            utils::init_logging("testlogs/binance/short_sell", "debug");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = Binance::new(exchange_config);
            let tp = TradingPair::new(&bex, "ADA/USDT");

            let current_price = match bex.get_price(Some(tp.symbol())) {
                Ok(vp) => vp.get(0).unwrap().price.parse::<f64>().unwrap(),
                Err(code) => {
                    panic!("{:?} failed to get price data: {:?}", tp.symbol(), code);
                }
            };

            match short_sell(&bex, &tp, 15.0, Some(current_price * 2.0)) {
                Ok(or) => {
                    debug!("{:?}", or);
                }
                Err(code) => {
                    panic!("failed to short sell: {:?}", code);
                }
            };
        }
    }

    #[test]
    fn trade_short() {
        let are_you_sure = false;

        if are_you_sure {
            utils::init_logging("testlogs/binance/trade_short", "debug");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = Binance::new(exchange_config);
            let tp = TradingPair::new(&bex, "ADA/USDT");

            let signal_msg = process_md::TradeThreadMsg {
                trade_action: None,
                trading_pair: None,
                quit: false,
                closing_price: 0.0,
            };

            trade(
                &bex,
                PositionType::Short,
                &tp,
                &signal_msg,
                None,
                order::OrderType::Market,
                None,
                0.0,
            );
        }
    }

    #[test]
    fn trade_long() {
        let are_you_sure = false;

        if are_you_sure {
            utils::init_logging("testlogs/binance/trade_long", "debug");
            let config_file = "conf/ct.ini".to_string();
            let (_, exchange_config) = config::new(&config_file);
            let bex = Binance::new(exchange_config);
            let tp = TradingPair::new(&bex, "ADA/USDT");

            let signal_msg = process_md::TradeThreadMsg {
                trade_action: None,
                trading_pair: None,
                quit: false,
                closing_price: 0.0,
            };

            trade(
                &bex,
                PositionType::Long,
                &tp,
                &signal_msg,
                None,
                order::OrderType::Market,
                None,
                0.0,
            );
        }
    }

    #[test]
    fn get_account_data2() {
        utils::init_logging("testlogs/binance/get_account_data2", "debug");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = Binance::new(exchange_config);
        let tp = TradingPair::new(&bex, "ADA/USDT");
        let ad = bex.get_isolated_margin_account_data(tp.symbol()).unwrap();

        info!("{:#?}", tp);
        info!("{:#?}", ad.assets[0]);
    }
}
