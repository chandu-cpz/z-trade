use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::types::{PositionSide, StrategyType};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub token: u32,
    pub side: PositionSide,
    pub strategy: StrategyType,
    pub entry_price: f64,
    pub current_price: f64,
    pub quantity: u32,
    pub entry_time: DateTime<Local>,
    pub highest_price: f64,
    pub lowest_price: f64,
    pub stop_loss: f64,
    pub target: f64,
}

impl Position {
    pub fn new(
        symbol: String,
        token: u32,
        side: PositionSide,
        strategy: StrategyType,
        entry_price: f64,
        quantity: u32,
        config: &Config,
    ) -> Self {
        let sl_amount = entry_price * (config.execution.stop_loss_pct / 100.0);
        let target_amount = sl_amount * config.execution.target_mult;

        let (stop_loss, target) = match side {
            PositionSide::Long => (entry_price - sl_amount, entry_price + target_amount),
            PositionSide::Short => (entry_price + sl_amount, entry_price - target_amount),
        };

        Self {
            symbol,
            token,
            side,
            strategy,
            entry_price,
            current_price: entry_price,
            quantity,
            entry_time: Local::now(),
            highest_price: entry_price,
            lowest_price: entry_price,
            stop_loss,
            target,
        }
    }

    #[inline]
    pub fn update_price(&mut self, price: f64) {
        self.current_price = price;
        if price > self.highest_price {
            self.highest_price = price;
        }
        if price < self.lowest_price {
            self.lowest_price = price;
        }
    }

    #[inline]
    pub fn unrealized_pnl(&self) -> f64 {
        let diff = if self.side == PositionSide::Long {
            self.current_price - self.entry_price
        } else {
            self.entry_price - self.current_price
        };
        diff * (self.quantity as f64)
    }

    #[inline]
    pub fn pnl_percent(&self) -> f64 {
        if self.entry_price == 0.0 {
            return 0.0;
        }
        let diff = if self.side == PositionSide::Long {
            self.current_price - self.entry_price
        } else {
            self.entry_price - self.current_price
        };
        (diff / self.entry_price) * 100.0
    }

    pub fn alpha_score(&self) -> f64 {
        let now = Local::now();
        let age_secs = now.signed_duration_since(self.entry_time).num_seconds();

        let pnl_pct = self.pnl_percent();
        let mut score = 50.0 + (pnl_pct * 200.0);

        let decay = (age_secs / 30) as f64 * 10.0;
        score -= decay;

        if age_secs > 120 && pnl_pct.abs() < 0.05 {
            score -= 30.0;
        }

        score.clamp(0.0, 100.0)
    }

    pub fn should_exit(&self, config: &Config) -> (bool, String) {
        let pnl_pct = self.pnl_percent();

        // 1. Hard Stop Loss
        let sl_hit = if self.side == PositionSide::Long {
            self.current_price <= self.stop_loss
        } else {
            self.current_price >= self.stop_loss
        };

        if sl_hit {
            return (true, format!("Stop Loss ({:.2}%)", pnl_pct));
        }

        // 2. Emergency Cut
        if pnl_pct <= config.execution.emergency_pct {
            return (true, format!("Emergency ({:.2}%)", pnl_pct));
        }

        // 3. High-Velocity Trailing Stop
        let trigger_price = if self.side == PositionSide::Long {
            self.entry_price * (1.0 + (config.execution.trail_stop_trigger_pct / 100.0))
        } else {
            self.entry_price * (1.0 - (config.execution.trail_stop_trigger_pct / 100.0))
        };

        let in_profit_zone = if self.side == PositionSide::Long {
            self.highest_price >= trigger_price
        } else {
            self.lowest_price <= trigger_price
        };

        if in_profit_zone {
            let sl_price = if self.side == PositionSide::Long {
                self.highest_price * (1.0 - (config.execution.trail_stop_back_pct / 100.0))
            } else {
                self.lowest_price * (1.0 + (config.execution.trail_stop_back_pct / 100.0))
            };

            let hit = if self.side == PositionSide::Long {
                self.current_price <= sl_price
            } else {
                self.current_price >= sl_price
            };

            if hit {
                return (true, format!("TrailSL ({:.2}%)", pnl_pct));
            }
        }

        (false, String::new())
    }
}
