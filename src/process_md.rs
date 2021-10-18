// Process market data (process_md.rs).
use crate::account_manager;
use crate::binance;
use crate::candlestick;
use crate::config;
use crate::ma;
use crate::order;
use crate::position;
use crate::tradingpair;

use math::round;
use std::collections::HashMap;
use std::{thread, time::Duration};
use websocket::{stream::sync::NetworkStream, sync::Client, ClientBuilder, OwnedMessage};

use serde_json;

use log::{debug, error, info};

use account_manager::{AccountManager, OrderQuantity};
use binance::Binance;
use config::{ExchangeConfig, StrategyConfig};
use position::PositionType;
use tradingpair::TradingPair;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TradeSignal {
    MaCross,
    MaTrendReversal,
    MACD,
}

#[derive(Debug)]
pub struct MarketDataTracker {
    pub slow_ma_data: ma::MAData,
    pub fast_ma_data: ma::MAData,
    pub macd: ma::MACD,

    pub desired_position: PositionType,

    // The signal type we are looking for.
    pub trade_signal: TradeSignal,

    // Previous candles, green or red?
    pub candle_color_history: Vec<candlestick::CandleColor>,

    // Exponential or simple MA.
    pub ema: bool,

    // Are we using BLVTs or not?
    pub bvlt: bool,

    // Market/Limit?
    pub order_type: order::OrderType,

    // % Away from the last close price we'll accept for a limit order.
    pub limit_offset: Option<u8>,

    // % Away from the average fill price that we want to set our stop loss at.
    pub stop_percent: Option<f64>,

    // % Gain we are happy to take a profit at.
    pub take_profit_percent: Option<f64>,

    // If trade_signal is TradeSignal::MACD then we want this number of green
    // candle before entering a position even if the signal has been triggered.
    // Same goes in the reverse direction for red candles.
    pub confirmation_candles: Option<u8>,

    // If we are using the macd as the primary indicator we might also have a
    // trend MA we need to be above in order to take a long position.
    pub macd_trend_ma: ma::MAData,
}

// The number of ticks away from the last closing price that we will accept.
static DEFAULT_LIMIT_RANGE: u8 = 2;

// Check & update if the last required number of candles are all green or all red.
fn trade_confirmation_via_previous_candles(
    mt: &mut MarketDataTracker,
    closing_price: f64,
    prev_closing_price: Option<f64>,
) -> Option<(bool, candlestick::CandleColor)> {
    if mt.confirmation_candles.is_some() {
        if let Some(prev_closing_price) = prev_closing_price {
            let num_confirmations = mt.confirmation_candles.unwrap();
            if num_confirmations as usize == mt.candle_color_history.capacity() {
                mt.candle_color_history.pop();
            }

            if prev_closing_price <= closing_price {
                mt.candle_color_history
                    .push(candlestick::CandleColor::GREEN);
            } else {
                mt.candle_color_history.push(candlestick::CandleColor::RED);
            }

            Some((
                mt.candle_color_history
                    .iter()
                    .all(|&color| color == mt.candle_color_history[0]),
                mt.candle_color_history[0],
            ))
        } else {
            // Color here doesn't matter.
            Some((false, candlestick::CandleColor::GREEN))
        }
    } else {
        None
    }
}

// Decide what we should do based on:
//
// TA
// Take profit override
// Current position
// Any extra confirmation signals
fn trading_decision(
    am: &AccountManager,
    trading_pair: &TradingPair,
    mt: &mut MarketDataTracker,
    closing_price: f64,
    prev_closing_price: Option<f64>,
) -> position::PositionType {
    let mut decision = PositionType::None;

    if trading_pair.get_bvlt_type().is_none() {
        decision = match mt.trade_signal {
            TradeSignal::MaCross => ma::trading_decision_ma_cross(&trading_pair, mt, closing_price),
            TradeSignal::MaTrendReversal => {
                ma::trading_decision_ma_trend_change(&trading_pair, mt, closing_price)
            }
            TradeSignal::MACD => ma::trading_decision_macd(&trading_pair, mt, closing_price),
        };

        // Update the list of previous candle colours and return if we've matched a number in
        // a row which are the same colour.
        let confirmation =
            trade_confirmation_via_previous_candles(mt, closing_price, prev_closing_price);

        // Check if we have confirmations in the right direction.
        let confirmed = if confirmation.is_some() {
            let confirmation = confirmation.unwrap();
            if decision == PositionType::Long {
                if confirmation.1 == candlestick::CandleColor::GREEN {
                    confirmation.0
                } else {
                    false
                }
            } else if decision == PositionType::Short {
                if confirmation.1 == candlestick::CandleColor::RED {
                    confirmation.0
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            // We don't need extra confirmation, just go for it.
            true
        };

        // Check if we have any open positions at the moment.
        let cur_position = am.get_position(trading_pair.symbol());
        let cur_position_type = match cur_position {
            Some((r#type, _, _)) => r#type,
            None => PositionType::None,
        };

        // Maybe override the signals if we hit a profit target.
        let take_profit_override = if mt.take_profit_percent.is_some() {
            match cur_position {
                Some((r#type, _qty, price)) => {
                    if r#type == PositionType::Long
                        && (closing_price
                            >= (price + ((price / 100.0) * mt.take_profit_percent.unwrap())))
                    {
                        true
                    } else {
                        false
                    }
                }

                None => false,
            }
        } else {
            false
        };

        if take_profit_override {
            if cur_position_type == PositionType::Long {
                decision = PositionType::Short;
                info!(
                    "{:#?} take profit hit: close price: {}, +{}% from entry, will sell",
                    trading_pair.symbol(),
                    closing_price,
                    mt.take_profit_percent.unwrap(),
                );
            }
        }

        if decision == cur_position_type {
            decision = PositionType::None;
        } else if (confirmed || take_profit_override) && decision != PositionType::None {
            info!(
                "{:#?} trade decision changed: {:#?} --> {:#?}",
                trading_pair.symbol(),
                cur_position_type,
                decision
            );
        }
    }

    return decision;
}

// Update all required TA indicators and check if we should make a trade, if we should
// then we submit an order to the AccountManager.
fn process_close_data(
    am: &AccountManager,
    trading_pair: &TradingPair,
    mt: &mut MarketDataTracker,
    closing_price: f64,
    prev_closing_price: Option<f64>,
    place_trades: bool,
) {
    // Compute the various technical indicators.
    match mt.trade_signal {
        TradeSignal::MaCross => {
            mt.slow_ma_data.compute(closing_price, mt.ema);
            mt.fast_ma_data.compute(closing_price, mt.ema);
        }
        TradeSignal::MaTrendReversal => {
            mt.fast_ma_data.compute(closing_price, mt.ema);
        }
        TradeSignal::MACD => {
            mt.macd.compute(closing_price);

            if mt.macd_trend_ma.num_candles > 0 {
                mt.macd_trend_ma.compute(closing_price, mt.ema);
            }
        }
    }

    if !place_trades {
        // If we just want to process the data then return now.
        return;
    }

    // Based on the latest TA and currently active position, compute the best new
    // position for us to take.
    let decision = trading_decision(am, trading_pair, mt, closing_price, prev_closing_price);

    match decision {
        PositionType::None => {}
        PositionType::Short | PositionType::Long => {
            // Compute the limit prices we are willing to accept for BUY/SELL orders.
            let limit_price = if mt.order_type == order::OrderType::Limit {
                let tick_increment = trading_pair.get_tick_size();
                if decision == PositionType::Long {
                    Some(round::floor(
                        closing_price
                            + (tick_increment
                                * mt.limit_offset
                                    .expect("limit offset is None but this is a limit order")
                                    as f64),
                        trading_pair.get_price_dps(),
                    ))
                } else {
                    Some(round::floor(
                        closing_price
                            - (tick_increment
                                * mt.limit_offset
                                    .expect("limit offset is None but this is a limit order")
                                    as f64),
                        trading_pair.get_price_dps(),
                    ))
                }
            } else {
                // Using MARKET orders.
                None
            };

            // Submit an order.
            am.spot_trade(
                trading_pair.clone(),
                decision,
                OrderQuantity::Percentage100,
                limit_price,
                mt.stop_percent,
            );
        }
    }
}

// Reconnect to the websocket stream.
fn reconnect_stream(
    ws_client: &mut ClientBuilder,
) -> Option<Client<Box<dyn NetworkStream + std::marker::Send>>> {
    let mut cur_try = 0;
    let max_tries = 5;
    while cur_try < max_tries {
        cur_try += 1;
        if let Ok(c) = ws_client.connect(None) {
            c.stream_ref()
                .as_tcp()
                .set_read_timeout(Some(Duration::new(60 * 1, 0)))
                .expect("failed to set read timeout");
            info!("connected to kline stream");
            return Some(c);
        } else {
            error!("failed to reconnect to kline stream");
            thread::sleep(Duration::from_millis(5000 * cur_try));
        }
    }

    None
}

// Process market data for the given trading pair and time frame, this processing
// may result in buy/sell signals with parameters being transmitted to the trading
// thread.
fn process_market_data_thread(
    ec: ExchangeConfig,
    log_dir: String,
    tp: TradingPair,
    time_frame: String,
    slow_ma: Option<u16>,
    fast_ma: Option<u16>,
    bvlt: bool,
    ema: bool,
    signal: TradeSignal,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
    stop_percent: Option<f64>,
    take_profit_percent: Option<f64>,
    confirmation_candles: Option<u8>,
    macd_trend_ma: Option<u16>,
) {
    info!(
        "starting {}ma compute thread for {:#?} using time frame {:#?} slow ma: {:#?}, fast ma {:#?}, signal: {:#?}",
        if ema { "e" } else { "s" },
        tp.symbol(),
        time_frame,
        slow_ma,
        fast_ma,
        signal,
    );

    let mut prev_closing_price: Option<f64> = None;
    let ec_am = ec.clone();
    let bex = Binance::new(ec);
    let am = AccountManager::new(ec_am, false, log_dir);
    let mut mt = MarketDataTracker {
        slow_ma_data: ma::MAData::new(slow_ma.unwrap_or(0)),
        fast_ma_data: ma::MAData::new(fast_ma.unwrap_or(0)),
        macd: ma::MACD::new(),
        desired_position: PositionType::None,
        candle_color_history: Vec::with_capacity(confirmation_candles.unwrap_or(0) as usize),
        ema: ema,
        bvlt: bvlt,
        trade_signal: signal,
        order_type: order_type,
        limit_offset: limit_offset,
        stop_percent: stop_percent,
        take_profit_percent: take_profit_percent,
        confirmation_candles: confirmation_candles,
        macd_trend_ma: ma::MAData::new(macd_trend_ma.unwrap_or(0)),
    };

    let mut req_params: HashMap<&str, &str> = HashMap::with_capacity(3);
    req_params.insert("symbol", tp.symbol());
    req_params.insert("interval", &time_frame);

    let historical_candles_required = if macd_trend_ma.unwrap_or(0) > slow_ma.unwrap_or(0) {
        macd_trend_ma.unwrap_or(0).to_string()
    } else {
        slow_ma.unwrap().to_string()
    };

    // Get the last candle sticks that we need to compute current moving averages.
    req_params.insert("limit", &historical_candles_required);
    if let Ok(st) = bex.get_server_time() {
        if let Ok(cd) = bex.get_cstick_data(&req_params) {
            let mut idx = 0;
            for stick in cd.iter() {
                if let Ok(closing_price) = stick.close_price.parse::<f64>() {
                    if st >= stick.close_time {
                        // Candle stick is closed, we can use it for ma calculation.
                        prev_closing_price = if idx > 0 {
                            Some(cd[idx - 1].close_price.parse::<f64>().unwrap())
                        } else {
                            None
                        };

                        idx += 1;

                        process_close_data(
                            &am,
                            &tp,
                            &mut mt,
                            closing_price,
                            prev_closing_price,
                            false,
                        );
                        prev_closing_price = Some(closing_price);
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
            am.exit();
            return;
        }
    } else {
        error!("{:?} failed to get server time, exiting", tp.symbol());
        am.exit();
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
    let mut conn = reconnect_stream(&mut ws_client).expect("failed to connect to stream");

    loop {
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
                                    &am,
                                    &tp,
                                    &mut mt,
                                    closing_price,
                                    prev_closing_price,
                                    true,
                                );
                            } else {
                                error!("failed to parse closing price: {}", cstick_data);
                            }
                        } else {
                            error!("failed to deserialize candlestick data: {}", s);
                        }
                    }

                    OwnedMessage::Ping(m) => match conn.send_message(&OwnedMessage::Pong(m)) {
                        Ok(_) => {
                            debug!("sent kline pong");
                        }
                        Err(e) => {
                            error!("failed to reply to ping message: {}", e);
                        }
                    },

                    OwnedMessage::Pong(_) => {
                        // I don't think we ever see pong messages.
                        debug!("got kline pong");
                    }

                    OwnedMessage::Binary(_) => {}

                    OwnedMessage::Close(e) => {
                        info!("disconnected from kline stream: {:?}", e);
                        match reconnect_stream(&mut ws_client) {
                            Some(c) => conn = c,
                            None => break,
                        };
                    }
                }
            }

            Err(e) => {
                error!("failed to receive data from the websocket: {}", e);
            }
        }
    }

    match conn.shutdown() {
        Ok(_) => {}
        Err(e) => {
            error!("failed to shutdown: {:?}", e);
        }
    }

    am.exit();
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
    log_dir: String,
    symset: String,
    time_frame: String,
    slow_ma: Option<u16>,
    fast_ma: Option<u16>,
    split_pct: u8,
    stop_percent: Option<f64>,
    take_profit_percent: Option<f64>,
    ema: bool,
    signal: TradeSignal,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
    confirmation_candles: Option<u8>,
    macd_trend_ma: Option<u16>,
) {
    info!("starting {}ma bvlt thread for: {} using time frame: {}, slow ma: {:?}, fast ma: {:?}, split {}%, stop_pct: {:?}%",
        if ema { "e" } else { "s" }, symset, time_frame, slow_ma, fast_ma, split_pct, stop_percent);

    let pairs: Vec<&str> = symset.split(":").collect();
    let n_ma_threads = pairs.len();
    assert!(n_ma_threads == 3);
    let mut handles = Vec::with_capacity(n_ma_threads);
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
        let ma_ec = ec.clone();
        let log_dir = log_dir.clone();
        let h = thread::spawn(move || {
            process_market_data_thread(
                ma_ec,
                log_dir,
                trading_pair,
                time_frame,
                slow_ma,
                fast_ma,
                true,
                ema,
                signal,
                order_type,
                limit_offset,
                stop_percent,
                take_profit_percent,
                confirmation_candles,
                macd_trend_ma,
            );
        });

        handles.push(h);
    }

    assert!(base_trading_pair.is_some());
}

// Spawns a data processing thread for processing market data and a trading thread
// for executing trades.
fn md_process_thread(
    ec: ExchangeConfig,
    log_dir: String,
    symbol: String,
    time_frame: String,
    slow_ma: Option<u16>,
    fast_ma: Option<u16>,
    split_pct: u8,
    stop_percent: Option<f64>,
    take_profit_percent: Option<f64>,
    ema: bool,
    signal: TradeSignal,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
    confirmation_candles: Option<u8>,
    macd_trend_ma: Option<u16>,
) {
    info!("starting {}ma basic thread for: {} using time frame: {}, slow ma: {:?}, fast ma: {:?}, split: {}%, stop_percent: {:?}%",
        if ema { "e" } else { "s" }, symbol, time_frame, slow_ma, fast_ma, split_pct, stop_percent);

    let bex = Binance::new(ec.clone());
    let trading_pair = TradingPair::new(&bex, &symbol);
    let tp = trading_pair.clone();
    let log_dir = log_dir.clone();
    let handle = thread::spawn(move || {
        process_market_data_thread(
            ec,
            log_dir,
            tp,
            time_frame,
            slow_ma,
            fast_ma,
            false,
            ema,
            signal,
            order_type,
            limit_offset,
            stop_percent,
            take_profit_percent,
            confirmation_candles,
            macd_trend_ma,
        );
    });

    // Sleep until all spawned threads exit.
    handle.join().unwrap();
}

pub fn run_strategy(strat_cfg: &StrategyConfig, log_dir: &str, ec: &ExchangeConfig) {
    // Parse configuration first.
    let slow_ma = match strat_cfg.members.get("SlowMA") {
        Some(slow_ma) => {
            let slow_ma = slow_ma
                .to_string()
                .parse::<u16>()
                .expect("SlowMA is not valid");

            Some(slow_ma)
        }

        None => None,
    };

    let fast_ma = match strat_cfg.members.get("FastMA") {
        Some(fast_ma) => {
            let fast_ma = fast_ma
                .to_string()
                .parse::<u16>()
                .expect("FastMA is not valid");

            Some(fast_ma)
        }

        None => None,
    };

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

    // Which signal to watch for.
    let signal = strat_cfg
        .members
        .get("Signal")
        .expect("Missing \"Signal\" configuration");

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

    // Stops with margin is not currently supported.
    let stop_percent = match strat_cfg.members.get("StopPercent") {
        Some(o) => {
            let stop_percent = o
                .to_string()
                .parse::<f64>()
                .expect("StopPercent should be >= 0.0 <= 100.0");
            if stop_percent <= 0.0 || stop_percent > 100.0 {
                panic!("StopPercent should be a percentage");
            }

            Some(stop_percent)
        }

        None => None,
    };

    // Take profit percent.
    let tp_percent = match strat_cfg.members.get("TakeProfitPercent") {
        Some(o) => {
            let tp_percent = o
                .to_string()
                .parse::<f64>()
                .expect("TakeProfitPercent should be >= 0.0 <= 100.0");
            if tp_percent <= 0.0 || tp_percent > 100.0 {
                panic!("TakeProfitPercent should be a percentage");
            }

            Some(tp_percent)
        }

        None => None,
    };

    let signal = {
        if signal.eq_ignore_ascii_case("trend") {
            TradeSignal::MaTrendReversal
        } else if signal.eq_ignore_ascii_case("cross") {
            TradeSignal::MaCross
        } else if signal.eq_ignore_ascii_case("macd") {
            TradeSignal::MACD
        } else {
            panic!("Unsupported signal: {}", signal);
        }
    };

    let confirmation_candles = match strat_cfg.members.get("ConfirmationCandles") {
        Some(confirmation_candles) => {
            let confirmation_candles = confirmation_candles
                .to_string()
                .parse::<u8>()
                .expect("ConfirmationCandles is not a number");
            if confirmation_candles > 10 {
                panic!("ConfirmationCandles < 10");
            }

            if signal != TradeSignal::MACD {
                panic!("ConfirmationCandles is set but macd is not configured as a strategy")
            }

            Some(confirmation_candles)
        }

        None => None,
    };

    let macd_trend_ma = match strat_cfg.members.get("MacdTrendMa") {
        Some(macd_trend_ma) => {
            let macd_trend_ma = macd_trend_ma
                .to_string()
                .parse::<u16>()
                .expect("MacdTrendMa is not a number");

            if signal != TradeSignal::MACD {
                panic!("MacdTrendMa is set but macd is not configured as a strategy")
            }

            Some(macd_trend_ma)
        }

        None => None,
    };

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
        let log_dir = log_dir.to_string();
        let h = if bvlt_mode {
            let symset = pair.to_string();
            thread::spawn(move || {
                md_bvlt_process_thread(
                    ec,
                    log_dir,
                    symset,
                    time_frame,
                    slow_ma,
                    fast_ma,
                    asset_split_pct,
                    stop_percent,
                    tp_percent,
                    ema,
                    signal,
                    order_type,
                    limit_range,
                    confirmation_candles,
                    macd_trend_ma,
                );
            })
        } else {
            thread::spawn(move || {
                md_process_thread(
                    ec,
                    log_dir.to_string(),
                    symbol,
                    time_frame,
                    slow_ma,
                    fast_ma,
                    asset_split_pct,
                    stop_percent,
                    tp_percent,
                    ema,
                    signal,
                    order_type,
                    limit_range,
                    confirmation_candles,
                    macd_trend_ma,
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
