use crate::binance::Binance;

#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
pub enum BvltType {
    BvltUp,   // An UP BVLT coin.
    BvltDown, // A DOWN BVLT coin.
}

#[derive(Debug, PartialEq, Clone, PartialOrd)]
pub struct TradingPair {
    name: String,
    symbol: String,
    sell_currency: String,
    buy_currency: String,
    bvlt_type: Option<BvltType>,
    price_dps: i8,     // Price decimal places.
    qty_dps: i8,       // Trade quantity decimal places.
    min_order: f64,    // Smallest amount we can buy/sell.
    tick_size: f64,    // Min price increment.
    min_notional: f64, // Min qty*price allowed.
}

impl TradingPair {
    pub fn new(bex: &Binance, n: &str) -> TradingPair {
        let buysell: Vec<&str> = n.split("/").collect();
        let symbol = String::from(n.replace("/", ""));
        let lot_size_filter = bex.get_lot_size_filter(&symbol).unwrap();
        let price_filter = bex.get_price_filter(&symbol).unwrap();
        let min_notional = bex.get_min_notional_filter(&symbol).unwrap();

        TradingPair {
            // EXAMPLE.
            name: String::from(n),                   // BTC/USDT.
            symbol: symbol,                          // BTCUSDT.
            sell_currency: String::from(buysell[0]), // BTC.
            buy_currency: String::from(buysell[1]),  // USDST.
            bvlt_type: if buysell[0].ends_with("UP") {
                // BTCUP.
                Some(BvltType::BvltUp)
            } else if buysell[0].ends_with("DOWN") {
                // BTCDOWN.
                Some(BvltType::BvltDown)
            } else {
                None
            },

            qty_dps: lot_size_filter.decimal_places,
            price_dps: price_filter.decimal_places,
            min_order: lot_size_filter.min_qty,
            tick_size: price_filter.tick_size,
            min_notional: min_notional,
        }
    }

    pub fn get_bvlt_type(&self) -> &Option<BvltType> {
        &self.bvlt_type
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    #[allow(dead_code)]
    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn sell_currency(&self) -> &str {
        &self.sell_currency
    }

    pub fn buy_currency(&self) -> &str {
        &self.buy_currency
    }

    pub fn get_price_dps(&self) -> i8 {
        self.price_dps
    }

    pub fn get_qty_dps(&self) -> i8 {
        self.qty_dps
    }

    #[allow(dead_code)]
    pub fn get_min_qty(&self) -> f64 {
        self.min_order
    }

    pub fn get_tick_size(&self) -> f64 {
        self.tick_size
    }

    pub fn get_min_notional(&self) -> f64 {
        self.min_notional
    }
}

#[cfg(test)]
mod tests {
    use crate::binance;
    use crate::config;
    use crate::tradingpair;
    use crate::utils;

    use log::info;

    #[test]
    fn basic() {
        utils::init_logging("testlogs/tradingpair/basic", "debug");
        let config_file = "conf/ct.ini".to_string();
        let (_, exchange_config) = config::new(&config_file);
        let bex = binance::Binance::new(exchange_config);
        let tp = tradingpair::TradingPair::new(&bex, "ADA/USDT");
        info!("{:#?}", tp);
    }
}
