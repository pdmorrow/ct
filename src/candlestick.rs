// structures and routines related to candle sticks.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CandleStick {
    pub open_time: u64,
    pub open_price: String,
    pub high_price: String,
    pub low_price: String,
    pub close_price: String,
    pub vol: String,
    pub close_time: u64,
    pub quote_asset_vol: String,
    pub num_trades: u64,
    pub tbba_vol: String,
    pub tbqa_vol: String,
    pub ignore: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct KLine {
    pub t: u64,    // Open time.
    pub T: u64,    // Close time.
    pub s: String, // Symbol.
    pub i: String, // Interval.
    f: u64,        // First trade ID
    L: u64,        // Last trade ID
    o: String,     // Open price
    c: String,     // Close price
    h: String,     // High price
    l: String,     // Low price
    v: String,     // Base asset volume
    n: u64,        // Number of trades
    x: bool,       // Is it closed.
    q: String,     // Quote asset volume
    V: String,     // Taker buy base asset volume
    Q: String,     // Taker buy quote asset volume
    B: String,     // Ignore
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct CandleStickWs {
    pub e: String, // Event type.
    pub E: String, // Event time
    pub s: String, // Symbol,
    pub k: KLine,  // KLine data.
}
