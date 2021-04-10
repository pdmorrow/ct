// Process market data (process_md.rs).
use crate::binance;
use crate::config;
use crate::ma;
use crate::order;
use crate::position;
use crate::trading;
use crate::tradingpair;

use std::collections::HashMap;
use std::sync::mpsc;
use std::{thread, time};
use websocket::{ClientBuilder, OwnedMessage};

use serde_json;

use log::{debug, error, info};

use binance::Binance;
use config::{ExchangeConfig, StrategyConfig};
use position::PositionType;
use tradingpair::TradingPair;

#[derive(Debug, Copy, Clone, PartialEq)]
enum TradeSignal {
    MaCross,
    MaTrendReversal,
    MACD,
}

#[derive(Debug, Clone)]
pub struct TradeThreadMsg {
    pub trade_action: Option<PositionType>, // Long, Short or None.
    pub trading_pair: Option<TradingPair>,  // The asset the message relates to.
    pub quit: bool,                         // Can be set to true to exit the trading thread.
    pub closing_price: f64,                 // Last candlestick closing price.
}

#[derive(Debug)]
pub struct MarketDataTracker {
    pub slow_ma_data: ma::MAData,
    pub fast_ma_data: ma::MAData,
    pub macd: ma::MACD,

    pub ma_cross_signal: PositionType,
    pub ma_trend_change_signal: PositionType,
    pub macd_signal: PositionType,

    // The overall decision if using a combination of the above signals.
    pub merged_signal: PositionType,
}

// The number of ticks away from the last closing price that we will
// accept
static DEFAULT_LIMIT_RANGE: u8 = 2;

// Compute the latest moving averages then check if we have a trade signal.
// If we do then signal to the trading thread. In BVLT mode we continuously
// send moving average data to the trading thread since those values are used
// as stop loss values.
fn process_close_data(
    trading_pair: &TradingPair,
    mt: &mut MarketDataTracker,
    closing_price: f64,
    tx_channel: Option<&mpsc::Sender<TradeThreadMsg>>,
    ma_mode: ma::MAMode,
    ema: bool,
    signals: &Vec<TradeSignal>,
) {
    // Update MAs.
    mt.slow_ma_data.compute(closing_price, ema);
    mt.fast_ma_data.compute(closing_price, ema);

    // Update MACD.
    mt.macd.compute(closing_price);

    let tx_channel = tx_channel.unwrap();

    // Enqueue a message to the trading thead to make a trade if there
    // is a signal.
    if trading_pair.get_bvlt_type().is_none() {
        assert!(ma_mode == ma::MAMode::BVLT || ma_mode == ma::MAMode::BASIC);

        // Check that all signals are satisfied.
        let decisions: Vec<PositionType> = signals
            .iter()
            .map(|sig| match sig {
                TradeSignal::MaCross => ma::trading_decision_ma_cross(&trading_pair, mt),
                TradeSignal::MaTrendReversal => {
                    ma::trading_decision_ma_trend_change(&trading_pair, mt)
                }
                TradeSignal::MACD => ma::trading_decision_macd(&trading_pair, mt),
            })
            .collect();

        let decision = &decisions[0];
        for d in &decisions {
            if d != decision {
                // Not all decisions matched
                return;
            }
        }

        if mt.merged_signal == *decision {
            // No change since last time.
            debug!(
                "{:#?} trade decision is unchanged: {:#?}",
                trading_pair.symbol(),
                decision
            );
            return;
        } else if *decision != PositionType::None {
            info!(
                "{:#?} trade decision changed: {:#?} --> {:#?}",
                trading_pair.symbol(),
                mt.merged_signal,
                decision
            );
            mt.merged_signal = *decision;
        }

        match decision {
            PositionType::None => {}
            PositionType::Short | PositionType::Long => {
                let msg = TradeThreadMsg {
                    trade_action: Some(*decision),
                    trading_pair: Some(trading_pair.clone()),
                    quit: false,
                    closing_price: closing_price,
                };
                match tx_channel.send(msg) {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            "{:#?} failed to send decision msg to trading thread: {:#?}",
                            trading_pair.symbol(),
                            e
                        );
                    }
                }
            }
        }
    } else {
        // This is one of the bvlt trading pairs (UP or DOWN), just send
        // the latest slow moving average value along with the associated
        // trading pair.
        assert!(ma_mode == ma::MAMode::BVLT);

        if mt.slow_ma_data.latest().is_some() {
            let msg = TradeThreadMsg {
                trade_action: None,
                trading_pair: Some(trading_pair.clone()),
                quit: false,
                closing_price: closing_price,
            };

            match tx_channel.send(msg) {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        "{:#?} failed to send blvt ma msg to trading thread: {:#?}",
                        trading_pair.symbol(),
                        e
                    );
                }
            }
        }
    }
}

// Send a quit message to the trading thread.
fn exit_trading_thread(tx_channel: &mpsc::Sender<TradeThreadMsg>) {
    let msg = TradeThreadMsg {
        trade_action: None,
        trading_pair: None,
        quit: true,
        closing_price: 0.0,
    };
    match tx_channel.send(msg) {
        Ok(_) => {}
        Err(e) => {
            error!("failed to send quit msg to trading thread: {:#?}", e);
        }
    }
}

// Process market data for the given trading pair and time frame, this processing
// may result in buy/sell signals with parameters being transmitted to the trading
// thread.
fn process_market_data_thread(
    ec: ExchangeConfig,
    tp: TradingPair,
    time_frame: String,
    slow_ma: u16,
    fast_ma: u16,
    tx_channel: mpsc::Sender<TradeThreadMsg>,
    ma_mode: ma::MAMode,
    ema: bool,
    signals: Vec<TradeSignal>,
) {
    info!(
        "starting {}ma compute thread for:\n\n{:#?} using time frame: {:#?}, slow sticks: {:#?}, fast sticks: {:#?}",
        if ema { "e" } else { "s" },
        tp,
        time_frame,
        slow_ma,
        fast_ma
    );

    let bex = Binance::new(ec);

    let mut mt = MarketDataTracker {
        slow_ma_data: ma::MAData::new(slow_ma),
        fast_ma_data: ma::MAData::new(fast_ma),
        macd: ma::MACD::new(),
        ma_cross_signal: PositionType::None,
        ma_trend_change_signal: PositionType::None,
        macd_signal: PositionType::None,
        merged_signal: PositionType::None,
    };

    let mut req_params: HashMap<&str, &str> = HashMap::with_capacity(3);
    req_params.insert("symbol", tp.symbol());
    req_params.insert("interval", &time_frame);

    // Get the last candle sticks that we need to compute current moving averages.
    let nslow_ma = (slow_ma + 1).to_string();
    req_params.insert("limit", &nslow_ma);
    if let Ok(st) = bex.get_server_time() {
        if let Ok(cd) = bex.get_cstick_data(&req_params) {
            for stick in cd.iter() {
                if let Ok(closing_price) = stick.close_price.parse::<f64>() {
                    if st >= stick.close_time {
                        // Candle stick is closed, we can use it for ma calculation.
                        process_close_data(
                            &tp,
                            &mut mt,
                            closing_price,
                            Some(&tx_channel),
                            ma_mode,
                            ema,
                            &signals,
                        );
                    }
                } else {
                    error!(
                        "failed to parse closing price {:?} to f64",
                        stick.close_price
                    );
                }
            }
        } else {
            error!("{:?} failed to get cstick data, exiting", tp.symbol());
            exit_trading_thread(&tx_channel);
            return;
        }
    } else {
        error!("{:?} failed to get server time, exiting", tp.symbol());
        exit_trading_thread(&tx_channel);
        return;
    }

    // We now switch over to the websocket interface to stream the candle
    // stick data from the exchange.
    let stream = format!(
        "wss://stream.binance.com:9443/ws/{}@kline_{}",
        tp.symbol().to_lowercase(),
        time_frame
    );
    let mut ws_client = ClientBuilder::new(&stream).unwrap();
    let mut conn = ws_client.connect(None).unwrap();
    info!("connected to {:?}", stream);

    let mut running = true;

    while running {
        match conn.recv_message() {
            Ok(om) => {
                match om {
                    OwnedMessage::Text(s) => {
                        let cstick: Result<serde_json::Value, _> = serde_json::from_str(&s);
                        if let Ok(cstick) = cstick {
                            let cstick_data: &serde_json::Value = &cstick["k"];
                            if cstick_data["x"] == false {
                                // Not closed, keep reading waiting.
                                continue;
                            }

                            let closing_price = cstick_data["c"]
                                .as_str()
                                .unwrap_or("0.0")
                                .parse::<f64>()
                                .unwrap_or(-1.0);

                            if closing_price > -1.0 {
                                process_close_data(
                                    &tp,
                                    &mut mt,
                                    closing_price,
                                    Some(&tx_channel),
                                    ma_mode,
                                    ema,
                                    &signals,
                                );
                            } else {
                                error!("failed to parse closing price: {:?}", cstick_data);
                            }
                        } else {
                            error!("failed to deserialize candlestick data: {:?}", s);
                        }
                    }

                    OwnedMessage::Ping(m) => match conn.send_message(&OwnedMessage::Pong(m)) {
                        Ok(_) => {
                            debug!("sent kline pong");
                        }
                        Err(e) => {
                            error!("failed to reply to ping message: {:?}", e);
                        }
                    },

                    OwnedMessage::Pong(_) => {
                        // I don't think we ever see pong messages.
                        debug!("got kline pong");
                    }

                    OwnedMessage::Binary(_) => {}

                    OwnedMessage::Close(e) => {
                        info!("disconnected {:?}", e);
                        let mut cur_try = 0;
                        running = false;
                        while cur_try < 5 {
                            cur_try += 1;
                            if let Ok(c) = ws_client.connect(None) {
                                conn = c;
                                running = true;
                                break;
                            } else {
                                error!("failed to reconnect to {:?}: {:?}", stream, e);
                                thread::sleep(time::Duration::from_millis(5000));
                            }
                        }
                    }
                }
            }

            Err(e) => {
                error!("error receiving data from the websocket: {:?}", e);
            }
        }
    }

    match conn.shutdown() {
        Ok(_) => {}
        Err(e) => {
            error!("failed to shutdown: {:?}", e);
        }
    }

    exit_trading_thread(&tx_channel);
}

// This function just spawns another 4 threads, 3 of those threads handle
// retrieving candlestick data and subsequent moving average computation
// for the following trading pairs.
//
// BASE, for example BTC/USDT.
// UP, for example BTCUP/USDT.
// DOWN: for example BTCDOWN/USDT.
//
// The 4th thread is the trading thread which acts on messages from any of
// the previous 3 threads.  The base thread will indicate trade opportunities
// and the UP and DOWN threads provide last slow MA values with which the
// trading thread can set stop loss orders.
fn md_bvlt_process_thread(
    ec: ExchangeConfig,
    symset: String,
    time_frame: String,
    slow_ma: u16,
    fast_ma: u16,
    split_pct: u8,
    stop_percent: f64,
    ema: bool,
    signals: &Vec<TradeSignal>,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
) {
    info!("starting {}ma bvlt thread for: {:#} using time frame: {:#}, slow sticks: {:#}, fast sticks: {:#}, split {:#?}%, stop_pct: {:#?}",
        if ema { "e" } else { "s" }, symset, time_frame, slow_ma, fast_ma, split_pct, stop_percent);

    let pairs: Vec<&str> = symset.split(":").collect();
    let n_ma_threads = pairs.len();
    assert!(n_ma_threads == 3);
    let mut handles = Vec::with_capacity(n_ma_threads);
    let (tx, rx) = mpsc::channel::<TradeThreadMsg>();
    let mut base_trading_pair: Option<TradingPair> = None;
    let bex = Binance::new(ec.clone());
    for symbol in pairs {
        let trading_pair = TradingPair::new(&bex, &symbol);
        // Find the base pair, we'll create another thread for
        // this one for making trades.
        if trading_pair.get_bvlt_type().is_none() {
            assert!(base_trading_pair.is_none());
            base_trading_pair = Some(trading_pair.clone());
        }

        let time_frame = time_frame.clone();
        let txc = tx.clone();
        let ma_ec = ec.clone();
        let sigs = signals.clone();
        let h = thread::spawn(move || {
            process_market_data_thread(
                ma_ec,
                trading_pair,
                time_frame,
                slow_ma,
                fast_ma,
                txc,
                ma::MAMode::BVLT,
                ema,
                sigs,
            );
        });

        handles.push(h);
    }

    assert!(base_trading_pair.is_some());
    // Create the trading thread.

    let ec = ec.clone();
    let trade_thread_handle = thread::spawn(move || {
        trading::bvlt_trading_thread(
            ec,
            base_trading_pair.unwrap(),
            rx,
            split_pct,
            stop_percent,
            order_type,
            limit_offset,
        );
    });

    // Sleep until all spawned threads exit.
    for h in handles {
        h.join().unwrap();
    }

    trade_thread_handle.join().unwrap();
}

// Spawns a data processing thread for processing market data and a trading thread
// for executing trades.
fn md_process_thread(
    ec: ExchangeConfig,
    symbol: String,
    time_frame: String,
    slow_ma: u16,
    fast_ma: u16,
    split_pct: u8,
    stop_percent: f64,
    ema: bool,
    signals: &Vec<TradeSignal>,
    short: bool,
    leverage: Option<f64>,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
) {
    info!("starting {}ma basic thread for: {:#} using time frame: {:#}, slow sticks: {:#}, fast sticks: {:#}, split {:#?}%, stop_percent: {:#?}, short selling: {:#?}",
        if ema { "e" } else { "s" }, symbol, time_frame, slow_ma, fast_ma, split_pct, stop_percent, short);

    let (tx, rx) = mpsc::channel::<TradeThreadMsg>();
    let bex = Binance::new(ec.clone());
    let trading_pair = TradingPair::new(&bex, &symbol);
    let ma_ec = ec.clone();
    let tp = trading_pair.clone();
    let sigs = signals.clone();
    let handle = thread::spawn(move || {
        process_market_data_thread(
            ec,
            tp,
            time_frame,
            slow_ma,
            fast_ma,
            tx,
            ma::MAMode::BASIC,
            ema,
            sigs,
        );
    });

    let tp = trading_pair.clone();
    let trade_thread_handle = thread::spawn(move || {
        trading::trading_thread(
            ma_ec,
            tp,
            rx,
            split_pct,
            stop_percent,
            short,
            leverage,
            order_type,
            limit_offset,
        );
    });

    // Sleep until all spawned threads exit.
    handle.join().unwrap();
    trade_thread_handle.join().unwrap();
}

// Main non BVLT based strategy, here we monitor various signals on
// a single trading pair then signal buy/sell to the trading thread.
pub fn run_strategy(strat_cfg: &StrategyConfig, ec: &ExchangeConfig) {
    // Parse configuration first.
    let slow_ma: u16 = strat_cfg
        .members
        .get("Slow")
        .expect("Missing \"Slow\" configuration")
        .parse()
        .expect("Failed to parse \"Slow\" configuration");

    let fast_ma: u16 = strat_cfg
        .members
        .get("Fast")
        .expect("Missing \"Fast\" configuration")
        .parse()
        .expect("Failed to parse \"Fast\" configuration");

    let time_frame = strat_cfg
        .members
        .get("TimeFrame")
        .expect("Missing \"TimeFrame\" configuration");

    let pairs: Vec<&str> = strat_cfg
        .members
        .get("Pairs")
        .expect("Missing \"Pairs\" configuration")
        .split(",")
        .collect();

    // BVLT Pair entries look like this:
    // Pairs=BTC/USDT:BTCUP/USDT:BTCDOWN/USDT
    let bvlt_mode = if pairs[0].find(':').is_some() {
        true
    } else {
        false
    };

    // Use simple moving averages or exponential.
    let ema: bool = strat_cfg
        .members
        .get("EMA")
        .unwrap_or(&"false".to_string())
        .parse::<bool>()
        .unwrap();

    // Which signals to watch for.
    let signals_cfg: Vec<&str> = strat_cfg
        .members
        .get("Signals")
        .expect("Missing \"Signals\" configuration")
        .split(",")
        .collect();

    // Enable short selling on down trend signals.
    let short: bool = strat_cfg
        .members
        .get("Short")
        .unwrap_or(&"false".to_string())
        .parse::<bool>()
        .unwrap();

    // BVLT always shorts right now.
    if bvlt_mode && !short {
        panic!("BVLT mode does not support Short=false");
    }

    // Market or limit orders to be used.
    let ot = match strat_cfg.members.get("OrderType") {
        Some(o) => o.to_string(),
        None => "Market".to_string(),
    };

    let order_type = match ot.as_str() {
        "Market" => order::OrderType::Market,
        "Limit" => order::OrderType::Limit,
        _ => {
            panic!(
                "Unexpected OrderType {:?}, use either Market (default) or Limit",
                ot
            );
        }
    };

    let limit_range = match order_type {
        order::OrderType::Limit => match strat_cfg.members.get("LimitOffset") {
            Some(o) => Some(
                o.to_string()
                    .parse::<u8>()
                    .expect("LimitOffset should be >= 0 < 256"),
            ),
            None => Some(DEFAULT_LIMIT_RANGE),
        },
        _ => None,
    };

    // Enable leveraged trading via the margin API.
    let leverage = match strat_cfg.members.get("Leverage") {
        Some(l) => {
            if l.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(l.parse::<f64>().expect("failed to parse {:?} as f64"))
            }
        }

        None => None,
    };

    // Stops with margin is not currently supported.
    let stop_percent = match strat_cfg.members.get("StopPercent") {
        Some(o) => {
            let stop_price = o
                .to_string()
                .parse::<f64>()
                .expect("StopPercent should be >= 0.0 <= 100.0");
            if stop_price < 0.0 || stop_price > 100.0 {
                panic!("StopPercent should be a percentage");
            }

            if leverage.is_some() || (short && !bvlt_mode) {
                panic!("StopPercent not currently supported when using leverage or shorts");
            }

            stop_price
        }

        None => 1.0,
    };

    // BVLT always shorts right now.
    if bvlt_mode && leverage.is_some() {
        panic!("BVLT mode is already using leverage, remove \"Leverage\" configuration");
    }

    let mut signals: Vec<TradeSignal> = Vec::with_capacity(signals_cfg.len());
    for signal in signals_cfg {
        if signal.eq_ignore_ascii_case("trend") {
            signals.push(TradeSignal::MaTrendReversal);
        } else if signal.eq_ignore_ascii_case("cross") {
            signals.push(TradeSignal::MaCross);
        } else if signal.eq_ignore_ascii_case("macd") {
            signals.push(TradeSignal::MACD);
        }
    }

    // If have one set of symbols then we invest 100% in that, if we
    // have 2 sets of symbols then each gets 50% and so on....
    let asset_split_pct: u8 = (100 / pairs.len()) as u8;

    // Create a thread per BASE/UP/DOWN tuple. For example if we wanted to
    // run MA_BVLT on ADA/USDT and BTC/USDT then our config would look
    // like this:
    //
    // Pairs=ADA/USDT:ADAUP/USDT:ADADOWN/USDT,BTC/USDT:BTCUP/USDT:BTCDOWN/USDT
    //
    // From this we would create a thread for handling ADA and a thread for
    // handling BTC. In turn those threads create yet more threads for computing
    // MAs for each trading pair.
    let nthreads = pairs.len();
    let mut handles = Vec::with_capacity(nthreads);
    for pair in pairs {
        let time_frame = time_frame.to_string();
        let ec = ec.clone();
        let symbol = pair.to_owned();
        let sigs = signals.clone();
        let h = if bvlt_mode {
            let symset = pair.to_string();
            thread::spawn(move || {
                md_bvlt_process_thread(
                    ec,
                    symset,
                    time_frame,
                    slow_ma,
                    fast_ma,
                    asset_split_pct,
                    stop_percent,
                    ema,
                    &sigs,
                    order_type,
                    limit_range,
                );
            })
        } else {
            thread::spawn(move || {
                md_process_thread(
                    ec,
                    symbol,
                    time_frame,
                    slow_ma,
                    fast_ma,
                    asset_split_pct,
                    stop_percent,
                    ema,
                    &sigs,
                    short,
                    leverage,
                    order_type,
                    limit_range,
                );
            })
        };

        handles.push(h);
    }

    // Sleep untill all of those threads exit.
    for h in handles {
        h.join().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::utils;

    #[test]
    fn ws_market_data_stream() {
        utils::init_logging("testlogs/ma/ws_market_data_stream", "info");
        let mut client = ClientBuilder::new("wss://stream.binance.com:9443/ws/btcusdt@kline_1m")
            .unwrap()
            .connect(None)
            .unwrap();

        match client.recv_message().unwrap() {
            OwnedMessage::Text(s) => {
                let j: serde_json::Value = serde_json::from_str(&s).unwrap();
                info!("{:?}", j);
            }
            OwnedMessage::Ping(_) | OwnedMessage::Pong(_) | OwnedMessage::Binary(_) => (),
            OwnedMessage::Close(_) => panic!("Disconnected"),
        }
    }
}
