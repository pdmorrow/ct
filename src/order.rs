use crate::binance::Binance;
use crate::position;
use crate::tradingpair::TradingPair;

use position::PositionType;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
    pub symbol: String,
    pub orderId: i64,
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

fn place_limit_order_internal(
    bex: &Binance,
    tp: &TradingPair,
    position: PositionType,
    qty: f64,
    price: f64,
) -> Result<OrderResponseAck, i64> {
    let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);
    order_params.insert("symbol", tp.symbol());
    order_params.insert("side", "SELL");
    order_params.insert("timeInForce", "GTC");
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

    bex.send_order(&mut order_params, false)
}

pub fn place_order_quantity(
    ex: &Binance,
    position: PositionType,
    tp: &TradingPair,
    quantity: f64,
    limit_price: Option<f64>,
) -> Result<OrderResponseAck, i64> {
    if limit_price.is_some() {
        place_limit_order_internal(ex, tp, position, quantity, limit_price.unwrap())
    } else {
        let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);
        order_params.insert("symbol", tp.symbol());

        if position == PositionType::Long {
            order_params.insert("side", "BUY");
        } else if position == PositionType::Short {
            order_params.insert("side", "SELL");
        } else {
            panic!("unknown requested position");
        }

        let q_str = quantity.to_string();
        order_params.insert("quantity", &q_str);
        order_params.insert("type", "MARKET");

        let ts_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let t = ts_now.to_string();
        order_params.insert("timestamp", &t);

        ex.send_order(&mut order_params, false)
    }
}

pub fn place_stop_limit(
    ex: &Binance,
    symbol: &str,
    quantity: f64,
    stop_trigger_price: f64,
    limit_price: f64,
) -> Result<OrderResponseAck, i64> {
    let mut order_params: HashMap<&str, &str> = HashMap::with_capacity(6);

    order_params.insert("symbol", symbol);
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

    ex.send_stop_order(&order_params)
}
