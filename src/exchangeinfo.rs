#[derive(Debug)]
pub struct PriceFilter {
    pub max_price: f64,
    pub min_price: f64,
    pub tick_size: f64,
    pub decimal_places: i8,
}

#[derive(Debug)]
pub struct LotSizeFilter {
    pub min_qty: f64,
    pub max_qty: f64,
    pub step_size: f64,
    pub decimal_places: i8,
}
