use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::types::{PositionSide, StrategyType};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: String,
    pub token: u32,
    pub side: PositionSide,
    pub strategy: StrategyType,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: u32,
    pub gross_pnl: f64,
    pub transaction_costs: f64,
    pub net_pnl: f64,
    pub entry_time: DateTime<Local>,
    pub exit_time: DateTime<Local>,
    pub exit_reason: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TradingStats {
    pub total_net_pnl: f64,
    pub total_gross_pnl: f64,
    pub total_fees: f64,
    pub realized_pnl: f64,
    pub trades_count: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub largest_win: f64,
    pub largest_loss: f64,
}

impl TradingStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_trade(&mut self, net_pnl: f64, gross_pnl: f64, fees: f64) {
        self.total_net_pnl += net_pnl;
        self.total_gross_pnl += gross_pnl;
        self.total_fees += fees;
        self.realized_pnl += net_pnl;
        self.trades_count += 1;

        if net_pnl >= 0.0 {
            self.winning_trades += 1;
            if net_pnl > self.largest_win {
                self.largest_win = net_pnl;
            }
        } else {
            self.losing_trades += 1;
            if net_pnl < self.largest_loss {
                self.largest_loss = net_pnl;
            }
        }
    }

    pub fn win_rate(&self) -> f64 {
        if self.trades_count == 0 {
            0.0
        } else {
            (self.winning_trades as f64 / self.trades_count as f64) * 100.0
        }
    }

    pub fn profit_factor(&self) -> f64 {
        if self.losing_trades == 0 {
            if self.winning_trades > 0 { 99.9 } else { 0.0 }
        } else {
            let total_wins = (self.winning_trades as f64) * self.largest_win.max(0.0) * 0.5;
            let total_losses = (self.losing_trades as f64) * self.largest_loss.abs() * 0.5;
            if total_losses > 0.0 { total_wins / total_losses } else { 0.0 }
        }
    }
}
