#![allow(dead_code)]

use chrono::{DateTime, Local};
use crate::config::Config;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PositionSide {
    Long,
    Short,
}

impl std::fmt::Display for PositionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionSide::Long => write!(f, "LONG"),
            PositionSide::Short => write!(f, "SHORT"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Strategy {
    // ScalpMom family
    VShapeDrop,      // N consec down-ticks + vol burst -> Short
    VShapeRip,       // N consec up-ticks + vol burst -> Long
    MomentumScalp,   // Rapid price accel + LTQ spike -> follow
    MeanReversion,   // Overextended spike/drop -> bet on return to mean

    // Alpha Pack
    OpeningSniper,   // ORB 5-min
    DepthPressure,   // Order Flow Imbalance (OFI)
    VWAPMagnet,      // StDev Mean Rev v2
    LiquiditySweep,  // Failed breakout reversal

    // Alpha Pack v2
    VolumeProfile,   // POC/LVN Breakout
    GapFade,         // Morning gap fill
    TrendPullback,   // Buy the dip in trend
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::VShapeDrop => write!(f, "VDrop"),
            Strategy::VShapeRip => write!(f, "VRip"),
            Strategy::MomentumScalp => write!(f, "MomSclp"),
            Strategy::MeanReversion => write!(f, "MeanRev"),
            Strategy::OpeningSniper => write!(f, "OpenSnip"),
            Strategy::DepthPressure => write!(f, "OFI"),
            Strategy::VWAPMagnet => write!(f, "VWAPMag"),
            Strategy::LiquiditySweep => write!(f, "LiqSwp"),
            Strategy::VolumeProfile => write!(f, "VolProf"),
            Strategy::GapFade => write!(f, "GapFade"),
            Strategy::TrendPullback => write!(f, "Pullbak"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PricePoint {
    pub price: f64,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub struct DisplaySignal {
    pub time: DateTime<Local>,
    pub symbol: String,
    pub side: PositionSide,
    pub strategy: Strategy,
    pub entered: bool,
    pub block_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Position {
    pub symbol: String,
    pub side: PositionSide,
    pub strategy: Strategy,
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
        side: PositionSide,
        strategy: Strategy,
        entry_price: f64,
        quantity: u32,
        config: &Config,
    ) -> Self {
        let sl_amount = entry_price * (config.stop_loss_pct / 100.0);
        let target_amount = sl_amount * config.target_mult;

        let (stop_loss, target) = match side {
            PositionSide::Long => (entry_price - sl_amount, entry_price + target_amount),
            PositionSide::Short => (entry_price + sl_amount, entry_price - target_amount),
        };

        Self {
            symbol,
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

    pub fn update_price(&mut self, price: f64) {
        self.current_price = price;
        if price > self.highest_price {
            self.highest_price = price;
        }
        if price < self.lowest_price {
            self.lowest_price = price;
        }
    }

    pub fn unrealized_pnl(&self) -> f64 {
        let diff = if self.side == PositionSide::Long {
            self.current_price - self.entry_price
        } else {
            self.entry_price - self.current_price
        };
        diff * (self.quantity as f64)
    }

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

        // 1. Base Score from PnL % (0.1% = 20 points)
        let pnl_pct = self.pnl_percent();
        let mut score = 50.0 + (pnl_pct * 200.0);

        // 2. Velocity Decay: -10 points every 30s ("Be quick or be dead")
        let decay = (age_secs / 30) as f64 * 10.0;
        score -= decay;

        // 3. Stagnation Penalty: if price hasn't moved in 120s, it's dead weight
        if age_secs > 120 && pnl_pct.abs() < 0.05 {
            score -= 30.0;
        }

        score.clamp(0.0, 100.0)
    }

    pub fn should_exit(&self, config: &Config) -> (bool, String) {
        let pnl_pct = self.pnl_percent();

        // 1. Hard Stops
        let sl_hit = if self.side == PositionSide::Long {
            self.current_price <= self.stop_loss
        } else {
            self.current_price >= self.stop_loss
        };

        if sl_hit {
            return (true, format!("Stop Loss ({:.2}%)", pnl_pct));
        }

        if pnl_pct <= config.emergency_pct {
            return (true, format!("Emergency ({:.2}%)", pnl_pct));
        }

        // 2. Trailing Stop (High velocity)
        let trigger_price = if self.side == PositionSide::Long {
            self.entry_price * (1.0 + (config.trail_stop_trigger_pct / 100.0))
        } else {
            self.entry_price * (1.0 - (config.trail_stop_trigger_pct / 100.0))
        };

        let in_profit_zone = if self.side == PositionSide::Long {
            self.highest_price >= trigger_price
        } else {
            self.lowest_price <= trigger_price
        };

        if in_profit_zone {
            let sl_price = if self.side == PositionSide::Long {
                self.highest_price * (1.0 - (config.trail_stop_back_pct / 100.0))
            } else {
                self.lowest_price * (1.0 + (config.trail_stop_back_pct / 100.0))
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

#[derive(Clone, Debug)]
pub struct Trade {
    pub symbol: String,
    pub side: PositionSide,
    pub strategy: Strategy,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: u32,
    pub pnl: f64,
    pub entry_time: DateTime<Local>,
    pub exit_time: DateTime<Local>,
    pub exit_reason: String,
}

#[derive(Default, Debug)]
pub struct TradingStats {
    pub total_pnl: f64,
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

    pub fn record_trade(&mut self, pnl: f64) {
        self.total_pnl += pnl;
        self.realized_pnl += pnl;
        self.trades_count += 1;

        if pnl >= 0.0 {
            self.winning_trades += 1;
            if pnl > self.largest_win {
                self.largest_win = pnl;
            }
        } else {
            self.losing_trades += 1;
            if pnl < self.largest_loss {
                self.largest_loss = pnl;
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
}
