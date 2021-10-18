use crate::position;
use crate::process_md;
use crate::tradingpair;

use position::PositionType;
use tradingpair::TradingPair;

use log::{debug, info};
use math::round;
use std::collections::VecDeque;

#[derive(Debug)]
pub struct MAData {
    latest: Option<f64>,                  // Current MA value.
    penultimate: Option<f64>,             // Previous MA value.
    penultimate_penultimate: Option<f64>, // Previous previous MA value.

    // MA accumulator data.
    pub acc: VecDeque<f64>,
    // Number of candles required before computing the average.
    pub num_candles: u16,
}

#[derive(Debug)]
pub struct MACD {
    pub ema12: MAData,
    pub ema26: MAData,
    pub signal: MAData,
    pub macd_latest: Option<f64>,
    pub macd_previous: Option<f64>,
}

impl MACD {
    pub fn new() -> Self {
        MACD {
            ema12: MAData::new(12),
            ema26: MAData::new(26),
            signal: MAData::new(9),
            macd_latest: None,
            macd_previous: None,
        }
    }

    pub fn compute(&mut self, close_price: f64) {
        self.ema12.compute(close_price, true);
        self.ema26.compute(close_price, true);

        if self.ema26.latest().is_some() {
            if self.macd_latest.is_some() {
                self.macd_previous = self.macd_latest;
            }

            let macd = self.ema12.latest().unwrap() - self.ema26.latest().unwrap();
            self.macd_latest = Some(macd);
            self.signal.compute(macd, true);
        }
    }
}

impl MAData {
    pub fn new(num_candles: u16) -> Self {
        MAData {
            acc: VecDeque::with_capacity(num_candles as usize),
            latest: None,
            penultimate: None,
            penultimate_penultimate: None,
            num_candles: num_candles,
        }
    }

    // Current simple moving average value.
    pub fn latest(&self) -> Option<f64> {
        self.latest
    }

    // Previous simple moving average value.
    pub fn penultimate(&self) -> Option<f64> {
        self.penultimate
    }

    // Previous previous simple moving average value.
    pub fn penultimate_penultimate(&self) -> Option<f64> {
        self.penultimate_penultimate
    }

    // Set new moving average value and make the old current
    // the penultimate.
    fn update(&mut self, new_ma: f64) {
        self.penultimate_penultimate = self.penultimate;
        self.penultimate = self.latest;
        self.latest = Some(new_ma);
    }

    // Compute the latest moving average value based on the close price.
    pub fn compute(&mut self, close_price: f64, ema: bool) {
        if self.num_candles == 0 {
            return;
        }

        if self.acc.len() == self.num_candles as usize {
            // Discard the oldest close price we saved.
            self.acc.pop_back();
        }

        // Add the newest close price to the accumulator vector.
        self.acc.push_front(close_price);
        if self.acc.len() == self.num_candles as usize {
            // We've got enough data to compute the MA.
            let mut acc_val = 0.0;

            for cp in self.acc.iter() {
                acc_val += cp;
            }

            let new_ma = acc_val / self.num_candles as f64;

            if ema {
                let prev_ema = match self.latest() {
                    Some(prev_ema) => prev_ema,
                    // No previous ema exists, use the current sma value as our starting value.
                    None => new_ma,
                };

                // https://www.investopedia.com/ask/answers/122314/what-exponential-moving-average-ema-formula-and-how-ema-calculated.asp
                let weight = 2.0 / (self.num_candles as f64 + 1.0);
                let ema = (close_price * weight) + (prev_ema * (1.0 - weight));
                self.update(ema);
            } else {
                self.update(new_ma);
            }
        }
    }
}

// MACD crossing signal line.
pub fn trading_decision_macd(
    tp: &TradingPair,
    mt: &process_md::MarketDataTracker,
    closing_price: f64,
) -> PositionType {
    if mt.macd.macd_latest.is_some()
        && mt.macd.signal.latest().is_some()
        && mt.macd.macd_previous.is_some()
    {
        let signal = mt.macd.signal.latest().unwrap();
        let macd_prev = mt.macd.macd_previous.unwrap();
        let macd = mt.macd.macd_latest.unwrap();

        debug!(
            "[MACD] {}, CLOSE: {}, MACD: {}, MACD_PREV: {}, SIGNAL: {}",
            tp.symbol(),
            closing_price,
            macd,
            macd_prev,
            signal,
        );

        if macd > signal && macd_prev < signal {
            if mt.macd_trend_ma.num_candles > 0 {
                let trend_ma_prev = mt.macd_trend_ma.penultimate();
                let trend_ma_latest = mt.macd_trend_ma.latest();

                if trend_ma_latest.is_some() && trend_ma_prev.is_some() {
                    if trend_ma_latest.unwrap() >= trend_ma_prev.unwrap() {
                        // Trending up, we can take this long.
                        info!(
                            "[BUY][MACD] {}, close: {}, signal: MACD({}) > SIGNAL({}) > MACD_PREV({}) TREND_UP_MA({})",
                            tp.symbol(),
                            closing_price,
                            macd,
                            signal,
                            macd_prev,
                            mt.macd_trend_ma.num_candles,
                        );
                        return position::PositionType::Long;
                    }
                }

                return position::PositionType::None;
            } else {
                info!(
                    "[BUY][MACD] {}, close: {}, signal: MACD({}) > SIGNAL({}) > MACD_PREV({})",
                    tp.symbol(),
                    closing_price,
                    macd,
                    signal,
                    macd_prev,
                );

                return position::PositionType::Long;
            }
        } else if macd < signal && macd_prev > signal {
            info!(
                "[SELL][MACD] {}, close: {}, signal: MACD({}) < SIGNAL({}) < MACD_PREV({})",
                tp.symbol(),
                closing_price,
                macd,
                signal,
                macd_prev,
            );

            return position::PositionType::Short;
        }
    }

    return PositionType::None;
}

// Trend reversal detection, returns:
// PositionType::Long if the fast ma starts to trend upwards.
// PositionType::Short if the fast ma starts to trend downwards.
pub fn trading_decision_ma_trend_change(
    tp: &TradingPair,
    mt: &process_md::MarketDataTracker,
    closing_price: f64,
) -> PositionType {
    if mt.slow_ma_data.latest().is_none()
        || mt.fast_ma_data.latest().is_none()
        || mt.fast_ma_data.penultimate().is_none()
        || mt.fast_ma_data.penultimate_penultimate().is_none()
    {
        return PositionType::None;
    }

    let c = mt.slow_ma_data.latest().unwrap();
    let p = mt.fast_ma_data.penultimate().unwrap();
    let pp = mt.fast_ma_data.penultimate_penultimate().unwrap();

    debug!(
        "[MA][TREND] {} CLOSE({}) FMA_PREV_PREV({}) FMA_PREV({}) FMA({})",
        tp.symbol(),
        closing_price,
        pp,
        p,
        c,
    );

    if c > p && p < pp {
        info!(
                "[BUY][TREND] {}, close: {}, signal: FMA({}) > FMA_PREV({}) and FMA_PREV({}) < FMA_PREV_PREV({})",
                tp.symbol(),
                closing_price,
                c,
                p,
                p,
                pp,
            );

        return PositionType::Long;
    } else if c < p && p > pp {
        info!(
                "[SELL][TREND] {}, close: {}, signal: FMA({}) < FMA_PREV({}) and FMA_PREV({}) > FMA_PREV_PREV({})",
                tp.symbol(),
                closing_price,
                c,
                p,
                p,
                pp,
            );

        return PositionType::Short;
    }

    return PositionType::None;
}

// Cross detection for moving averages, returns:
// PositionType::Long if the fast ma crosses the slow from below.
// PositionType::Short if the fast ma crosses the slow from above.
pub fn trading_decision_ma_cross(
    tp: &TradingPair,
    mt: &process_md::MarketDataTracker,
    closing_price: f64,
) -> PositionType {
    if mt.fast_ma_data.latest().is_some()
        && mt.slow_ma_data.latest().is_some()
        && mt.fast_ma_data.penultimate().is_some()
    {
        // We have data to make a decision.
        let dps = tp.get_price_dps();
        let f_ma_latest_val = round::floor(mt.fast_ma_data.latest().unwrap(), dps);
        let f_ma_prev_val = round::floor(mt.fast_ma_data.penultimate().unwrap(), dps);
        let s_ma_latest_val = round::floor(mt.slow_ma_data.latest().unwrap(), dps);

        debug!(
            "[MA][CROSS] {:#?} CLOSE({}) FMA({}) SMA({})",
            tp.symbol(),
            closing_price,
            f_ma_latest_val,
            s_ma_latest_val,
        );

        if f_ma_latest_val > s_ma_latest_val && f_ma_prev_val < s_ma_latest_val {
            // Fast moving average is above the slow moving average
            info!(
                "[BUY][CROSS] {:#?}, close: {}, signal: FMA({}) > SMA({} > FMA_PREV({})",
                tp.symbol(),
                closing_price,
                f_ma_latest_val,
                s_ma_latest_val,
                f_ma_prev_val,
            );

            return PositionType::Long;
        } else if f_ma_latest_val < s_ma_latest_val && f_ma_prev_val > s_ma_latest_val {
            // Fast moving average is below the slow moving average.
            info!(
                "[SELL][CROSS] {:#?}, close: {}, signal: FMA({}) < SMA({}) < FMA_PREV({})",
                tp.symbol(),
                closing_price,
                f_ma_latest_val,
                s_ma_latest_val,
                f_ma_prev_val,
            );

            return PositionType::Short;
        }
    }

    // No signal indicated or no change detected.
    return PositionType::None;
}
