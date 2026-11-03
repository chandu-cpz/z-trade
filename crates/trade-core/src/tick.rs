use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// High-throughput packed binary representation of a single market tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tick {
    pub symbol: String,
    pub token: u32,
    pub price: f64,
    pub volume: u64,
    pub total_buy_qty: u64,
    pub total_sell_qty: u64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub last_trade_qty: u64,
    pub timestamp: DateTime<Local>,
}

impl Tick {
    #[inline(always)]
    pub fn spread(&self) -> f64 {
        if self.best_bid > 0.0 && self.best_ask > 0.0 {
            (self.best_ask - self.best_bid).max(0.0)
        } else {
            0.0
        }
    }

    #[inline(always)]
    pub fn spread_pct(&self) -> f64 {
        if self.best_bid > 0.0 {
            (self.spread() / self.best_bid) * 100.0
        } else {
            0.0
        }
    }

    #[inline(always)]
    pub fn order_book_imbalance(&self) -> f64 {
        let total = (self.total_buy_qty + self.total_sell_qty) as f64;
        if total > 0.0 {
            (self.total_buy_qty as f64 - self.total_sell_qty as f64) / total
        } else {
            0.0
        }
    }
}
