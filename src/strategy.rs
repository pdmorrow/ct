use bitmask_enum::{bitmask};

#[bitmask]
#[derive(Debug)]
pub enum StrategyTypes {
    ScalpMA,
    Invalid,
}

pub fn from_str_cs (strat_list: &str) -> StrategyTypes {
    let strat_bitmask = StrategyTypes::none();
    let strats: Vec<&str> = strat_list.split(",").collect();
    for s in &strats {
        if s.eq_ignore_ascii_case("ScalpMA") {
            strat_bitmask.or(StrategyTypes::ScalpMA);
        } else {
            panic!("unsupported strategy: {:?}", s);
        }
    }

    strat_bitmask
}
