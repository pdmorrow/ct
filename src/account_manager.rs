use crate::balance;
use crate::binance;
use crate::config;
use crate::order;
use crate::position;
use crate::tradingpair;
use crate::utils;

use chrono;
use log::{debug, error, info};
use math::round;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::{thread, time::Duration};
use websocket::{stream::sync::NetworkStream, sync::Client, ClientBuilder, OwnedMessage};

use balance::Balance;
use binance::Binance;
use config::ExchangeConfig;
use position::{Position, PositionType};
use tradingpair::TradingPair;

#[derive(Debug, Clone)]
enum OrderType {
    Market,
    Limit,
}

#[derive(Debug, Clone)]
pub enum OrderQuantity {
    #[allow(dead_code)]
    Exact(f64),
    #[allow(dead_code)]
    PercentageAmount(u8),
    #[allow(dead_code)]
    Percentage25,
    #[allow(dead_code)]
    Percentage50,
    #[allow(dead_code)]
    Percentage75,
    Percentage100,
}

#[derive(Debug, Clone)]
struct OrderMsg {
    tp: TradingPair,
    order_type: OrderType,
    position: PositionType,
    quantity: OrderQuantity,
    limit_price: Option<f64>,
    stop_percent: Option<f64>,
    quit: bool,
}

pub struct AccountManager {
    tx_channel: mpsc::Sender<OrderMsg>,
    positions: Arc<Mutex<HashMap<String, Position>>>,
}

impl AccountManager {
    pub fn get_position(&self, symbol: &str) -> Option<(PositionType, f64, f64)> {
        let pos = self.positions.lock().unwrap();
        match pos.get(symbol) {
            Some(p) => {
                return Some((p.r#type, p.qty, p.price));
            }
            None => {
                return None;
            }
        }
    }

    pub fn exit(&self) {}
}

// Compute the cost of a trade in USDT.
fn compute_commision_usdt(
    bex: &Binance,
    commission_asset: &str,
    commission: f64,
    price: f64,
    symbol: &str,
) -> f64 {
    if commission_asset.eq("USDT") {
        // No conversion required.
        commission
    } else if symbol.starts_with(commission_asset) {
        // Commision asset is the same as the thing we are trading, for example
        // we are trading BTCUSDT and the commision asset is BTC.
        commission * price
    } else {
        // Need to ask the exchange what the current price of commission asset is in usdt,
        // for example we used BNB to pay the commision. So we need to get the current price
        // of BNBUSDST.
        let usdtsymbol = format!("{}USDT", commission_asset);
        match bex.get_price(&usdtsymbol) {
            Ok(p) => commission * p.price.parse::<f64>().unwrap(),
            Err(code) => {
                error!(
                    "failed to compute commision for {}: {}",
                    commission_asset, code
                );
                0.0
            }
        }
    }
}

// Receive orders from other threads, send those orders to the exchange.
fn order_thread(
    ec: ExchangeConfig,
    ad: Arc<Mutex<HashMap<String, Balance>>>,
    rx_channel: mpsc::Receiver<OrderMsg>,
    event_cv: Arc<(Mutex<bool>, Condvar)>,
    stop_percent: Arc<Mutex<Option<f64>>>,
    _margin: bool,
) {
    let bex = Binance::new(ec);

    loop {
        debug!("waiting for message");
        let msg = match rx_channel.recv() {
            Ok(msg) => {
                if msg.quit {
                    info!("quit signal received, exiting");
                }
                msg
            }
            Err(err) => {
                error!("failed to recv() message: {:?}", err);
                continue;
            }
        };

        // If there are open orders on this symbol then cancel them
        // and re-queue this order from the event thread after the orders
        // have been cancelled.
        if let Ok(orders) = bex.get_open_orders(msg.tp.symbol()) {
            if orders.as_array().unwrap().len() > 0 {
                let (lock, cvar) = &*event_cv;
                let mut waiting = lock.lock().unwrap();
                *waiting = true;
                match bex.cancel_all_orders(msg.tp.symbol()) {
                    Ok(_) => {
                        info!("waiting on order cancellation completion");
                        let mut retry = 0;
                        while *waiting && retry < 4 {
                            waiting = cvar
                                .wait_timeout(waiting, Duration::from_secs(5))
                                .unwrap()
                                .0;
                            retry += 1;
                        }

                        if *waiting {
                            *waiting = false;
                            info!("gave up waiting for order cancellation");
                        }
                    }
                    Err(code) => {
                        error!(
                            "failed to cancel open orders on {}: {}",
                            msg.tp.symbol(),
                            code
                        );
                    }
                }
            }
        }

        // What funds do we have available for this trade.
        let asset = if msg.position == PositionType::Long {
            msg.tp.buy_currency()
        } else {
            msg.tp.sell_currency()
        };
        let (free, locked) = match ad.lock().unwrap().get_mut(asset) {
            Some(balance) => (balance.free, balance.locked),
            None => {
                info!("no local balance for {:?}", asset);
                continue;
            }
        };

        debug!(
            "balance for {:?}: free: {:?} locked: {:?}",
            asset, free, locked
        );

        // Check the current or request price to see if we can actually trade
        // this quantity.
        let current_price = match msg.order_type {
            OrderType::Limit => msg.limit_price.unwrap(),
            OrderType::Market => match bex.get_price(msg.tp.symbol()) {
                Ok(p) => p.price.parse::<f64>().unwrap(),
                Err(code) => {
                    error!("failed to get price of {:?}: {:?}", msg.tp, code);
                    continue;
                }
            },
        };

        // Get the amount of the asset we want to trade.
        let max_qty = if msg.position == PositionType::Long {
            // How many can we buy?
            free / current_price
        } else {
            // What do we have to sell?
            free
        };
        let requested_qty = round::floor(
            if msg.position == PositionType::Long {
                // What percentage of our spend assets do we want to use?
                match msg.quantity {
                    OrderQuantity::Exact(q) => q,
                    OrderQuantity::PercentageAmount(q) => {
                        assert!(q <= 100);
                        max_qty * (q as f64 / 100.0)
                    }
                    OrderQuantity::Percentage100 => max_qty,
                    OrderQuantity::Percentage75 => max_qty * (3.0 / 4.0),
                    OrderQuantity::Percentage50 => max_qty * (1.0 / 2.0),
                    OrderQuantity::Percentage25 => max_qty * (1.0 / 4.0),
                }
            } else {
                // Always sell all.
                // TODO: If we sell first then we'll ignore the percentage stuff, so our first
                max_qty
            },
            msg.tp.get_qty_dps(),
        );

        let cost = current_price * requested_qty;
        let min_notional = msg.tp.get_min_notional();
        if cost < min_notional {
            info!(
                "order thread, trade value {} is too small, min is {}",
                cost, min_notional
            );
            continue;
        }

        let mut stop_pct = stop_percent.lock().unwrap();
        if msg.stop_percent.is_some() {
            *stop_pct = Some(msg.stop_percent.unwrap());
        }

        match order::place_order_quantity(
            &bex,
            msg.position,
            &msg.tp,
            requested_qty,
            msg.limit_price,
        ) {
            Ok(ack) => {
                info!(
                    "submitted {} order with id {} for {}",
                    if msg.position == PositionType::Long {
                        "BUY"
                    } else {
                        "SELL"
                    },
                    ack.orderId,
                    ack.symbol
                );
            }
            Err(code) => {
                error!("failed to place order: {:?} {:?}", code, msg);
            }
        }
    }
}

// Submit a stop loss sell order at the current price - 'stop_percent' or the current price.
fn submit_stop_order(
    bex: &Binance,
    stop_percent: f64,
    price_paid: f64,
    price_dps: u8,
    qty: f64,
    symbol: &str,
) {
    // Stop trigger price is a percentage delta from the price we paid.
    let stop_trigger_price = round::floor(
        price_paid - ((price_paid * stop_percent) / 100.0),
        price_dps as i8,
    );
    let stop_limit_price = stop_trigger_price;
    match order::place_stop_limit(&bex, symbol, qty, stop_trigger_price, stop_limit_price) {
        Ok(ack) => {
            info!(
                "submitted stop loss order of {} {} @ {:.*} with id {} for {}",
                qty, symbol, price_dps as usize, stop_trigger_price, ack.orderId, ack.symbol
            );
        }
        Err(code) => {
            error!("failed to submit stop loss: {}", code);
        }
    }
}

fn connect_stream(lk: &str) -> Option<Client<Box<dyn NetworkStream + std::marker::Send>>> {
    let stream = format!("wss://stream.binance.com:9443/ws/{}", lk);
    let mut ws_client = ClientBuilder::new(&stream).unwrap();
    let conn = match ws_client.connect(None) {
        Ok(c) => c,
        Err(err) => {
            error!("failed to connect to stream: {}", err);
            return None;
        }
    };
    // Set a read timeout of 30mins so we can fallback and keep the
    // server talking to us.
    conn.stream_ref()
        .as_tcp()
        .set_read_timeout(Some(Duration::new(60 * 30, 0)))
        .expect("failed to set read timeout");
    info!("connected to user data stream");
    Some(conn)
}

// Thread which handles events on the websocket, those events can be:
//
// Balance updates.
// Account updates (withdraw/deposit).
// Trade execution report.
fn event_thread(
    ec: ExchangeConfig,
    ad: Arc<Mutex<HashMap<String, Balance>>>,
    positions: Arc<Mutex<HashMap<String, Position>>>,
    _order_tx: mpsc::Sender<OrderMsg>,
    ready_barrier: Arc<Barrier>,
    event_cv: Arc<(Mutex<bool>, Condvar)>,
    stop_percent: Arc<Mutex<Option<f64>>>,
    log_dir: String,
) {
    let bex = Binance::new(ec);

    // Populate local view of balances, this is updated when events occur.
    let remote_ad = match bex.get_account_data() {
        Ok(remote_ad) => remote_ad,
        Err(code) => {
            panic!("failed to get account data: {:?}", code);
        }
    };

    // Create logfile for trade completion and balance data.
    // TODO: add tp or lk suffix.
    let utc_timestamp = chrono::offset::Utc::now().to_string().replace(" ", "_");
    let mut pb = PathBuf::from(&log_dir);
    pb.push(format!("tradelog_{}.txt", utc_timestamp));
    let mut tradelog = match File::create(pb.as_path()) {
        Err(code) => panic!("couldn't open {}: {}", pb.display(), code),
        Ok(f) => f,
    };

    for balance in remote_ad.balances {
        if balance.free != 0.0 || balance.locked != 0.0 {
            writeln!(
                &mut tradelog,
                "balance,{},free,{},locked,{}",
                balance.asset, balance.free, balance.locked
            )
            .unwrap();
            let mut ad = ad.lock().unwrap();
            ad.insert(balance.asset.to_string(), balance);
        }
    }

    let mut lk = match bex.create_listen_key() {
        Ok(lk) => lk,
        Err(code) => {
            panic!("could not create listen key: {:?}", code);
        }
    };

    let mut conn = connect_stream(&lk).unwrap();

    // Wait till we are connected before we allow anything else to happen.
    ready_barrier.wait();

    let mut running = true;
    let mut cancelled_order = false;
    let mut trade_buy_price: Option<f64> = None;
    let mut ave_trade_buy_price: Option<f64> = None;
    let mut trade_sell_price: Option<f64> = None;
    let mut trade_commission_usdt: Option<f64> = None;
    let mut total_buy_quantity: Option<f64> = None;
    let mut price_dps: Option<u8> = None;
    let mut cuml_pnl: f64 = 0.0;
    let mut cuml_commission: f64 = 0.0;
    let mut fills = 0;
    let mut buy_is_filled = false;
    let mut buy_symbol = String::from("NOSYMBOL");

    while running {
        // TODO: Need timeout on this.
        match conn.recv_message() {
            Ok(om) => {
                match om {
                    OwnedMessage::Text(s) => {
                        let payload: Result<serde_json::Value, _> = serde_json::from_str(&s);
                        if let Ok(payload) = payload {
                            let et: &serde_json::Value = &payload["e"];

                            match et.as_str().unwrap() {
                                "balanceUpdate" => {
                                    let asset = payload["a"].as_str().unwrap();
                                    let delta = payload["d"]
                                        .as_str()
                                        .unwrap()
                                        .parse::<f64>()
                                        .unwrap_or(0.0);
                                    debug!("balance update: {:?} {:?}", asset, delta);
                                    let mut ad_w = ad.lock().unwrap();
                                    let entry = ad_w.get_mut(asset);
                                    if entry.is_some() {
                                        let mut b = entry.unwrap();
                                        b.free += delta;
                                        let msg = format!(
                                            "balance:{},free:{},locked:{}",
                                            asset, b.free, b.locked
                                        );
                                        info!("{}", msg);
                                        writeln!(&mut tradelog, "{}", msg).unwrap();
                                    } else {
                                        error!("balanceUpdate for unknown asset {:?}", asset);
                                    }
                                }
                                "outboundAccountPosition" => {
                                    let ap: &serde_json::Value = &payload["B"];
                                    let updated_balances = ap.as_array().unwrap();
                                    for b in updated_balances {
                                        let asset = b["a"].as_str().unwrap();
                                        let new_free =
                                            b["f"].as_str().unwrap().parse::<f64>().unwrap();
                                        let new_locked =
                                            b["l"].as_str().unwrap().parse::<f64>().unwrap();
                                        let mut ad_w = ad.lock().unwrap();
                                        ad_w.insert(
                                            asset.to_string(),
                                            Balance {
                                                asset: asset.to_string(),
                                                free: new_free,
                                                locked: new_locked,
                                            },
                                        );

                                        let msg = format!(
                                            "balance:{},free:{},locked:{}",
                                            asset, new_free, new_locked,
                                        );
                                        info!("{}", msg);
                                        writeln!(&mut tradelog, "{}", msg).unwrap();

                                        if buy_is_filled {
                                            buy_is_filled = false;
                                            // After the account update we might need to place a
                                            // stop loss.
                                            let stp = stop_percent.lock().unwrap();
                                            if stp.is_some() {
                                                let stp = stp.unwrap();
                                                submit_stop_order(
                                                    &bex,
                                                    stp,
                                                    ave_trade_buy_price.unwrap(),
                                                    price_dps.unwrap(),
                                                    total_buy_quantity.unwrap(),
                                                    &buy_symbol,
                                                );
                                            }
                                        }
                                    }

                                    if cancelled_order {
                                        // Balances are updated after order cancellation,
                                        // unblock the order thread so it can complete the
                                        // current order using latest balance data.
                                        let (lock, cvar) = &*event_cv;
                                        let mut waiting_on_cancel = lock.lock().unwrap();
                                        *waiting_on_cancel = false;
                                        cvar.notify_one();
                                        cancelled_order = false;
                                    }
                                }
                                "executionReport" => {
                                    let symbol = &payload["s"].as_str().unwrap().to_string();
                                    let id = &payload["i"].as_u64().unwrap().to_string();
                                    let side = &payload["S"].as_str().unwrap().to_string();
                                    let ot = &payload["o"].as_str().unwrap().to_string();
                                    let tenforce = &payload["f"].as_str().unwrap().to_string();
                                    let status = &payload["X"].as_str().unwrap().to_string();
                                    let filled_qty = &payload["l"].as_str().unwrap().to_string();
                                    let cuml_filled_qty =
                                        &payload["z"].as_str().unwrap().to_string();
                                    let price = &payload["L"].as_str().unwrap().to_string();
                                    let commission = &payload["n"].as_str().unwrap().to_string();
                                    let commission_asset =
                                        &payload["N"].as_str().unwrap_or("NONE").to_string();

                                    let msg = format!("order:{},symbol:{},status:{},side:{},type:{},time_enforce:{},qty:{},price:{},commision_asset:{},commision:{}",
                                        id, symbol, status, side, ot, tenforce, filled_qty, price, commission_asset, commission);
                                    info!("{}", msg);
                                    writeln!(&mut tradelog, "{}", msg).unwrap();

                                    if status.eq("CANCELED") {
                                        cancelled_order = true;
                                        fills = 0;

                                        // Remove from the positions hashmap.
                                        let mut pm = positions.lock().unwrap();
                                        pm.remove(&buy_symbol);

                                        if !ot.eq("STOP_LOSS_LIMIT") {
                                            trade_buy_price = None;
                                            trade_sell_price = None;
                                            ave_trade_buy_price = None;
                                            trade_commission_usdt = None;
                                        }
                                    } else if status.eq("FILLED") {
                                        fills += 1;

                                        let commission = commission.parse::<f64>().unwrap();

                                        trade_commission_usdt = Some(
                                            trade_commission_usdt.unwrap_or(0.0)
                                                + compute_commision_usdt(
                                                    &bex,
                                                    &commission_asset,
                                                    commission,
                                                    price.parse::<f64>().unwrap(),
                                                    &symbol,
                                                ),
                                        );

                                        cuml_commission += trade_commission_usdt.unwrap();

                                        if side.eq("BUY") {
                                            // Record buy completly filled, save some things here so that we
                                            // can submit a stop loss when our account update comes in.
                                            price_dps = Some(utils::decimal_places(price));
                                            let price = price.parse::<f64>().unwrap();
                                            trade_buy_price =
                                                Some(price + trade_buy_price.unwrap_or(0.0));
                                            ave_trade_buy_price =
                                                Some(trade_buy_price.unwrap() / fills as f64);
                                            total_buy_quantity =
                                                Some(cuml_filled_qty.parse::<f64>().unwrap());
                                            buy_symbol = String::from(symbol);
                                            fills = 0;
                                            trade_buy_price = None;
                                            buy_is_filled = true;

                                            // Insert into the positions hashmap.
                                            let mut pm = positions.lock().unwrap();
                                            assert!(!pm.contains_key(&buy_symbol));
                                            pm.insert(
                                                String::from(&buy_symbol),
                                                Position {
                                                    price: ave_trade_buy_price.unwrap(),
                                                    qty: total_buy_quantity.unwrap(),
                                                    r#type: PositionType::Long,
                                                },
                                            );
                                        } else {
                                            // SELL.
                                            let price = price.parse::<f64>().unwrap();
                                            trade_sell_price =
                                                Some(price + trade_sell_price.unwrap_or(0.0));
                                            let asp = trade_sell_price.unwrap() / fills as f64;

                                            // Remove from the positions hashmap.
                                            let mut pm = positions.lock().unwrap();
                                            pm.remove(&buy_symbol);

                                            if ave_trade_buy_price.is_some() {
                                                let abp = ave_trade_buy_price.unwrap();
                                                let price_delta = asp - abp; // May be negative.
                                                let price_delta_pct = (price_delta / abp) * 100.0;
                                                let qty = cuml_filled_qty.parse::<f64>().unwrap();
                                                let commission = trade_commission_usdt.unwrap();
                                                let pnl = (qty * price_delta) - commission;
                                                cuml_pnl += pnl;
                                                let msg = format!(
                                                    "symbol:{},result:{},pnl:{:.2},cuml_pnl:{:.2},price_delta_pct:{:.*}%,price_delta:{:.*},commision_usdt:{:.2},cuml_pl_usdt:{:.2},cuml_commision_usdt:{:.2}",
                                                    symbol,
                                                    if abp < asp { "WIN" } else { "LOSS" },
                                                    pnl,
                                                    cuml_pnl,
                                                    price_dps.unwrap() as usize,
                                                    price_delta_pct,
                                                    price_dps.unwrap() as usize,
                                                    price_delta,
                                                    trade_commission_usdt.unwrap(),
                                                    cuml_pnl,
                                                    cuml_commission,
                                                );
                                                info!("{}", msg);
                                                writeln!(&mut tradelog, "{}", msg).unwrap();
                                            }

                                            fills = 0;
                                            trade_sell_price = None;
                                            trade_commission_usdt = None;
                                        }
                                    } else if status.eq("PARTIALLY_FILLED") {
                                        fills += 1;
                                        let price = price.parse::<f64>().unwrap();

                                        let commission = commission.parse::<f64>().unwrap();

                                        trade_commission_usdt = Some(
                                            trade_commission_usdt.unwrap_or(0.0)
                                                + compute_commision_usdt(
                                                    &bex,
                                                    &commission_asset,
                                                    commission,
                                                    price,
                                                    &symbol,
                                                ),
                                        );

                                        cuml_commission += trade_commission_usdt.unwrap();

                                        if side.eq("BUY") {
                                            trade_buy_price =
                                                Some(price + trade_buy_price.unwrap_or(0.0));
                                        } else {
                                            trade_sell_price =
                                                Some(price + trade_sell_price.unwrap_or(0.0));
                                        }
                                    }
                                }
                                _ => {
                                    error!("unexpected event type: {:?}", et.to_string());
                                }
                            }
                        } else {
                            error!("failed to deserialize user data payload: {:?}", s);
                        }
                    }

                    OwnedMessage::Ping(m) => match conn.send_message(&OwnedMessage::Pong(m)) {
                        Ok(_) => {
                            debug!("sent userdata pong");
                        }
                        Err(e) => {
                            error!("failed to reply to ping message: {:?}", e);
                        }
                    },

                    OwnedMessage::Pong(_) => {
                        // I don't think we ever see pong messages.
                        debug!("got userdata pong");
                    }

                    OwnedMessage::Binary(_) => {}

                    OwnedMessage::Close(e) => {
                        info!("userdata stream disconnected {:?}", e);
                        let mut cur_try = 0;
                        running = false;

                        while cur_try < 5 {
                            cur_try += 1;
                            match bex.delete_listen_key(lk.clone()) {
                                Ok(_) => {
                                    debug!("deleted listen key {}", lk);
                                }
                                Err(code) => {
                                    error!("failed to delete listen key {}: {}", lk, code);
                                    continue;
                                }
                            }

                            lk = match bex.create_listen_key() {
                                Ok(lk) => lk,
                                Err(code) => {
                                    error!("could not create listen key: {:?}", code);
                                    continue;
                                }
                            };

                            conn = match connect_stream(&lk) {
                                Some(c) => c,
                                None => {
                                    continue;
                                }
                            };

                            continue;
                        }
                    }
                }

                // Keep our connection alive.
                if let Err(code) = bex.ping_listen_key(lk.clone()) {
                    error!("failed to ping listen key stream: {:?}", code);
                }
            }

            Err(e) => {
                match e {
                    websocket::WebSocketError::NoDataAvailable => {
                        // Assume a timeout, ping the server.
                        info!("NoDataAvailable timeout, ping the server");
                        if let Err(code) = bex.ping_listen_key(lk.clone()) {
                            error!("failed to ping listen key stream: {:?}", code);
                        }
                    }
                    _ => {
                        error!("error receiving data from the websocket: {:?}", e);
                    }
                }
            }
        }
    }
}

impl AccountManager {
    pub fn new(ec: ExchangeConfig, margin: bool, log_dir: String) -> AccountManager {
        let (order_tx, order_rx) = mpsc::channel::<OrderMsg>();
        let ad = Arc::new(Mutex::new(HashMap::new()));
        let positions = Arc::new(Mutex::new(HashMap::new()));

        let ec = ec.clone();
        let ec2 = ec.clone();

        let ad_events = Arc::clone(&ad);
        let ad_orders = Arc::clone(&ad);

        let positions_events = Arc::clone(&positions);

        let events_tx = order_tx.clone();

        let ready_barrier = Arc::new(Barrier::new(2));
        let event_thread_ready_barrier = Arc::clone(&ready_barrier);

        let order_completed_cv = Arc::new((Mutex::new(true), Condvar::new()));
        let event_thread_order_completed_cv = Arc::clone(&order_completed_cv);

        let stop_percent_ot = Arc::new(Mutex::new(None));
        let stop_percent_et = Arc::clone(&stop_percent_ot);

        let log_dir = log_dir.clone();

        thread::spawn(move || {
            event_thread(
                ec,
                ad_events,
                positions_events,
                events_tx,
                event_thread_ready_barrier,
                event_thread_order_completed_cv,
                stop_percent_et,
                log_dir.to_string(),
            )
        });
        thread::spawn(move || {
            order_thread(
                ec2,
                ad_orders,
                order_rx,
                order_completed_cv,
                stop_percent_ot,
                margin,
            )
        });

        // Wait until the event thread is ready to go.
        ready_barrier.wait();

        AccountManager {
            tx_channel: order_tx,
            positions: Arc::clone(&positions),
        }
    }

    fn submit_order(&self, om: OrderMsg) {
        self.tx_channel.send(om).unwrap();
    }

    // Queue a long position to the order thread.
    pub fn spot_trade(
        &self,
        tp: TradingPair,
        position: PositionType,
        quantity: OrderQuantity,
        limit_price: Option<f64>,
        stop_percent: Option<f64>,
    ) {
        let om = OrderMsg {
            tp: tp,
            order_type: if limit_price.is_none() {
                OrderType::Market
            } else {
                OrderType::Limit
            },
            position: position,
            quantity: quantity,
            limit_price: limit_price,
            stop_percent: stop_percent,
            quit: false,
        };

        self.submit_order(om);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::utils;

    use log::info;

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
