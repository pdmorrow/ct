#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PositionType {
    Long,
    Short,
    None,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Position {
    pub r#type: PositionType,
    pub qty: f64,
    pub price: f64,
}
