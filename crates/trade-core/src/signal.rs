use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::types::{PositionSide, StrategyType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub symbol: String,
    pub token: u32,
    pub price: f64,
    pub side: PositionSide,
    pub strategy: StrategyType,
    pub timestamp: DateTime<Local>,
    pub alpha_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySignal {
    pub time: DateTime<Local>,
    pub symbol: String,
    pub side: PositionSide,
    pub strategy: StrategyType,
    pub entered: bool,
    pub block_reason: Option<String>,
}
