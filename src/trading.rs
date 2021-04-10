use crate::binance;
use crate::config;
use crate::margin;
use crate::order;
use crate::position;
use crate::process_md;
use crate::tradingpair;

use binance::Binance;
use config::ExchangeConfig;
use position::PositionType;
use tradingpair::TradingPair;

use log::{debug, error, info};
use std::sync::mpsc;

fn spot_trade(
    bex: &Binance,
    desired_position: PositionType,
    tp: &TradingPair,
    signal_msg: &process_md::TradeThreadMsg,
    split_pct: u8,
    order_type: order::OrderType,
    limit_offset: Option<u8>,
    stop_percent: f64,
) {
    if desired_position == PositionType::Long {
        debug!("[BUY] {:#?}: trade thread, got signal to buy", tp.symbol());

        // Cancel any open orders on the pair (i.e. cancel any stops)
        match bex.cancel_all_orders(tp.symbol()) {
            Ok(_) => {
                debug!("[CANCEL] cancelled all open orders on {:#?}", tp.symbol());
            }
            Err(_) => {}
        }

        let stop_price =
            Some(signal_msg.closing_price - (signal_msg.closing_price * (stop_percent / 100.0)));

        let limit_price = match order_type {
            order::OrderType::Market => None,
            order::OrderType::Limit => {
                assert!(limit_offset.is_some());
                Some(signal_msg.closing_price + (limit_offset.unwrap() as f64 * tp.get_tick_size()))
            }
        };

        match order::buy(&bex, &tp, stop_price, split_pct, limit_price) {
            Ok(_) => {
                debug!("[BUY] {:#?} complete", tp.symbol());
            }

            Err(e) => {
                error!("[BUY] {:#?} buy failed: {:#?}", tp.symbol(), e);
            }
        }
    } else if desired_position == PositionType::Short {
        debug!(
            "[SELL] {:#?}: trade thread, got signal to sell",
            tp.symbol()
        );

        // Cancel any open orders on the pair (i.e. cancel any stops)
        match bex.cancel_all_orders(tp.symbol()) {
            Ok(_) => {
                debug!("[CANCEL] cancelled all open orders on {:#?}", tp.symbol());
            }
            Err(_) => {}
        }

        let limit_price = match order_type {
            order::OrderType::Market => None,
            order::OrderType::Limit => {
                assert!(limit_offset.is_some());
                Some(signal_msg.closing_price - (limit_offset.unwrap() as f64 * tp.get_tick_size()))
            }
        };

        match order::sell(&bex, &tp, limit_price) {
            Ok(_) => {
                debug!("[SELL] {:#?} complete", tp.symbol(),);
            }

            Err(e) => {
                error!("[SELL] {:#?} failed: {:#?}", tp.symbol(), e);
            }
        }
    } else {
        assert!(desired_position != PositionType::None);
    }
}

// Basic trading thread.
//
// This thread sleeps untill it receives a message to perform
// some trading action.
pub fn trading_thread(
    ec: ExchangeConfig,
    tp: TradingPair,
    rx_channel: mpsc::Receiver<process_md::TradeThreadMsg>,
    split_pct: u8,          // How much currency do we want use out of our total.
    stop_percent: f64,      // Stop loss order is a percent of our purchase price.
    shorting_enabled: bool, // Optionally go short on down trends.
    leverage: Option<f64>,  // Leverage to use.
    order_type: order::OrderType,
    limit_offset: Option<u8>, // If using limit orders, price target offset from last closing price.
) {
    let bex = Binance::new(ec);

    loop {
        debug!("{:#?} trade thread, waiting for message", tp.symbol());
        let msg = rx_channel.recv().unwrap();
        if msg.quit {
            info!(
                "{:#?} trade thread exiting, cancelling all open orders and selling everything",
                tp.symbol()
            );

            // TODO, add support for dealing with margin.
            order::cancel_and_sell_all(&bex, &tp);

            // Exit the thread message.
            return;
        }

        // Long(BUY) or Short(SELL).
        let ta = msg.trade_action.unwrap();
        assert!(ta != PositionType::None);

        if shorting_enabled || leverage.is_some() {
            margin::trade(
                &bex,
                ta,
                &tp,
                &msg,
                leverage,
                order_type,
                limit_offset,
                stop_percent,
            );
        } else {
            spot_trade(
                &bex,
                ta,
                &tp,
                &msg,
                split_pct,
                order_type,
                limit_offset,
                stop_percent,
            );
        }
    }
}

// BVLT trading thread.
//
// We need to wait until all the following conditions are met:
//
// 1) Got a trade signal on the base pair (for example BTC/USDT)
// 2) Got latest MA values and names of the other 2 pairs, for example
//    BTCUP/USDT and BTCDOWN/USDT.
//
// Stop will be placed if use_stops is true.
pub fn bvlt_trading_thread(
    ec: ExchangeConfig,
    base_tp: TradingPair,
    rx_channel: mpsc::Receiver<process_md::TradeThreadMsg>,
    split_pct: u8,
    stop_percent: f64,
    order_type: order::OrderType,
    limit_offset: Option<u8>, // If using limit orders, price target offset from last closing price.
) {
    let bex = Binance::new(ec);

    let mut bvlt_type: Option<tradingpair::BvltType> = None;
    let mut ltp: Option<TradingPair> = None;
    let mut stp: Option<TradingPair> = None;

    loop {
        debug!("{:#?} trade thread, waiting for message", base_tp.symbol());
        let msg = rx_channel.recv().unwrap();
        if msg.quit {
            info!(
                "{:#?} trade thread exiting, cancelling all open orders and selling everything",
                base_tp.symbol()
            );

            // Cancel open orders & sell everything.
            match ltp {
                Some(tp) => {
                    order::cancel_and_sell_all(&bex, &tp);
                }

                None => {}
            }

            match stp {
                Some(tp) => {
                    order::cancel_and_sell_all(&bex, &tp);
                }

                None => {}
            }

            // Exit the thread message.
            return;
        }

        if let Some(ta) = msg.trade_action {
            if ta == PositionType::Long {
                // This is a signal on the base pair, we should buy the
                // UP token as long as we have a stop price. The stop price
                // is sent by the bvlt up ma compute thread and is handled
                // here **.
                bvlt_type = Some(tradingpair::BvltType::BvltUp);
                debug!(
                    "[BUY] {:#?}: trade thread, got signal to buy: {:#?}",
                    base_tp.symbol(),
                    bvlt_type.unwrap(),
                );
            } else if ta == PositionType::Short {
                // This is a signal on the base pair, we should buy the
                // DOWN token as long as we have a stop price. The stop price
                // is sent by the bvlt down ma compute thread and is handled
                // here **.
                bvlt_type = Some(tradingpair::BvltType::BvltDown);
                debug!(
                    "[SELL] {:#?}: trade thread, got signal to buy {:#?}",
                    base_tp.symbol(),
                    bvlt_type.unwrap(),
                );
            } else {
                assert!(ta != PositionType::None)
            }
        } else {
            // ** here.
            //
            // Store the stop price of both UP and DOWN coins as well
            // as their trading pair structs.

            assert!(msg.trading_pair.is_some());
            let tp = msg.trading_pair.unwrap();
            let bt = tp.get_bvlt_type();
            assert!(bt.is_some());

            match bt.unwrap() {
                tradingpair::BvltType::BvltUp => {
                    ltp = Some(tp);
                }

                tradingpair::BvltType::BvltDown => {
                    stp = Some(tp);
                }
            }
        }

        if bvlt_type.is_some() && ltp.is_some() && stp.is_some() {
            let stp = stp.unwrap();
            let ltp = ltp.unwrap();
            // We've got all we need to buy/sell.
            match bvlt_type.unwrap() {
                tradingpair::BvltType::BvltUp => {
                    // Sell the DOWN coin and buy the UP coin.
                    let limit_price = match order_type {
                        order::OrderType::Market => None,
                        order::OrderType::Limit => {
                            assert!(limit_offset.is_some());
                            Some(
                                msg.closing_price
                                    + (limit_offset.unwrap() as f64 * stp.get_tick_size()),
                            )
                        }
                    };

                    match order::sell(&bex, &stp, limit_price) {
                        Ok(_) => {
                            debug!(
                                "[SELL] {:#?}: {:#?} complete",
                                base_tp.symbol(),
                                stp.symbol()
                            );
                        }
                        Err(e) => {
                            error!(
                                "[SELL] {:#?}: {:#?} failed: {:#?}",
                                base_tp.symbol(),
                                stp.symbol(),
                                e
                            );
                        }
                    }

                    // Cancel any open orders on the long pair (i.e. cancel any stops)
                    match bex.cancel_all_orders(ltp.symbol()) {
                        Ok(_) => {
                            debug!("[CANCEL] cancelled all open orders on {:#?}", ltp.symbol());
                        }
                        Err(_) => {}
                    }

                    let limit_price = match order_type {
                        order::OrderType::Market => None,
                        order::OrderType::Limit => {
                            assert!(limit_offset.is_some());
                            Some(
                                msg.closing_price
                                    - (limit_offset.unwrap() as f64 * ltp.get_tick_size()),
                            )
                        }
                    };

                    match order::buy(&bex, &ltp, None, split_pct, limit_price) {
                        Ok(ave_fill) => {
                            debug!(
                                "[BUY] {:#?}: {:#?} complete",
                                base_tp.symbol(),
                                ltp.symbol(),
                            );

                            order::place_stop_loss(&bex, &ave_fill, &ltp, stop_percent);
                        }

                        Err(e) => {
                            error!(
                                "[BUY] {:#?}: {:#?} buy failed: {:#?}",
                                base_tp.symbol(),
                                ltp.symbol(),
                                e
                            );
                        }
                    }
                }

                tradingpair::BvltType::BvltDown => {
                    // Sell the UP coin and buy the DOWN coin.
                    match order::sell(&bex, &ltp, None) {
                        Ok(_) => {
                            debug!(
                                "[SELL] {:#?}: {:#?} complete",
                                base_tp.symbol(),
                                ltp.symbol(),
                            );
                        }

                        Err(e) => {
                            error!(
                                "[SELL] {:#?}: {:#?} failed: {:#?}",
                                base_tp.symbol(),
                                ltp.symbol(),
                                e
                            );
                        }
                    }

                    match order::buy(&bex, &stp, None, split_pct, None) {
                        Ok(ave_fill) => {
                            debug!(
                                "[BUY] {:#?}: {:#?} complete",
                                base_tp.symbol(),
                                stp.symbol(),
                            );

                            order::place_stop_loss(&bex, &ave_fill, &stp, stop_percent);
                        }

                        Err(e) => {
                            debug!(
                                "[BUY] {:#?}: {:#?} failed: {:#?}",
                                base_tp.symbol(),
                                stp.symbol(),
                                e
                            );
                        }
                    }
                }
            }
        } else {
            // Waiting for more messages.
            continue;
        }

        // We completed some trades, reset the data for next time around.
        ltp = None;
        stp = None;
        bvlt_type = None;
    }
}
