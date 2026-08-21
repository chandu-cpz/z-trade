use chrono::{DateTime, Local};

// ═══════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════
pub const SCALP_MAX_POSITIONS: usize = 20;       
pub const SCALP_MAX_LOSS: f64 = 20000.0;          
pub const SCALP_MAX_PRICE: f64 = 5000.0;         

pub const SCALP_MIN_TICKS: usize = 20;           
pub const SCALP_CONFIRMATION: usize = 1;         
pub const SCALP_SL_PERCENT: f64 = 0.40;          
pub const SCALP_TARGET_MULT: f64 = 1.5;          
pub const TRAIL_STOP_TRIGGER_PCT: f64 = 0.50;   
pub const TRAIL_STOP_BACK_PCT: f64 = 0.20;      
pub const SCALP_EMERGENCY_PCT: f64 = -1.00;     

pub const SCALP_ALERT_COOLDOWN: i64 = 5;        

pub const MAX_SPREAD_PCT: f64 = 0.10;             
pub const LOSS_COOLDOWN_SECS: i64 = 300;          
pub const TREND_WINDOW_SECS: i64 = 180;           

pub const SESSION_START_HOUR: u32 = 9;
pub const SESSION_START_MIN: u32 = 15;

// Proprietary Alpha Strategies
pub const SPOOF_OFI_RATIO: f64 = 5.0;            // Minimum OFI imbalance to assume retail trap
pub const SPOOF_SLOPE_THRESH: f64 = 0.015;       // Required counter-trend slope (price moving AGAINST OFI)
pub const Q_ACCUM_MAX_VARIANCE: f64 = 0.0005;    // 0.05% max price variance over window for accumulation
pub const Q_ACCUM_LTQ_MULT: f64 = 5.0;           // 5x LTQ surge on breakout

// ═══════════════════════════════════════════════════════════════════════════
// SNIPER CONFLUENCE CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════

pub const CONFLUENCE_SLOPE: f64 = 0.015;           
pub const CONFLUENCE_OFI_RATIO: f64 = 2.5;        
pub const CONFLUENCE_VOL_MULT: f64 = 3.5;        
pub const MICRO_BREAKOUT_TICKS: usize = 120;     

// Conviction Scoring (Dynamic Capital Deployment)
pub const CONVICTION_BASE_THRESHOLD: f64 = 90.0;        
pub const CONVICTION_MAX_THRESHOLD: f64 = 98.0;         
pub const CONVICTION_SWAP_ADVANTAGE: f64 = 80.0; // Disabled by making unreachable

// ═══════════════════════════════════════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════════════════════════════════════

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
pub struct PricePoint {
    pub price: f64,
    pub timestamp: DateTime<Local>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TradingMode {
    ScalpMom,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PositionSide {
    Long,
    Short,
}

impl std::fmt::Display for PositionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PositionSide::Long => write!(f, "LONG"),
            PositionSide::Short => write!(f, "SHORT"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Strategy {
    AlphaConfluenceLong,
    AlphaConfluenceShort,
    LiquiditySweepReversal,
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Strategy::AlphaConfluenceLong  => write!(f, "ConfluLong"),
            Strategy::AlphaConfluenceShort => write!(f, "ConfluShrt"),
            Strategy::LiquiditySweepReversal => write!(f, "SweepRev"),
        }
    }
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
    pub fn new(symbol: String, side: PositionSide, strategy: Strategy, entry_price: f64, quantity: u32, _mode: TradingMode) -> Self {
        let sl_pct = SCALP_SL_PERCENT;
        let target_mult = SCALP_TARGET_MULT;
        let sl_amount = entry_price * (sl_pct / 100.0);
        let target_amount = sl_amount * target_mult;
        
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
        if self.entry_price == 0.0 { return 0.0; }
        let diff = if self.side == PositionSide::Long {
            self.current_price - self.entry_price
        } else {
            self.entry_price - self.current_price
        };
        (diff / self.entry_price) * 100.0
    }

    pub fn alpha_score(&self) -> f64 {
        let now = chrono::Local::now();
        let age_secs = now.signed_duration_since(self.entry_time).num_seconds();
        
        // 1. Base Score from PnL % (Very aggressive)
        let pnl_pct = self.pnl_percent();
        let mut score = 50.0 + (pnl_pct * 200.0); // 0.1% = 20 points
        
        // 2. Aggressive Velocity Decay
        // -10 points every 30s. "Be quick or be dead"
        let decay = (age_secs / 30) as f64 * 10.0;
        score -= decay;

        // 3. Harsh Stagnation Penalty
        // If price hasn't moved meaningfully in 120s, it's dead weight
        if age_secs > 120 && pnl_pct.abs() < 0.05 {
            score -= 30.0; 
        }

        score.clamp(0.0, 100.0)
    }

    pub fn should_exit(&self, _mode: TradingMode) -> (bool, String) {
        let pnl_pct = self.pnl_percent();
        let _secs_held = Local::now().signed_duration_since(self.entry_time).num_seconds();
        let _in_profit = pnl_pct > 0.0;
        
        // High-Velocity Exit Logic
        let (trailing_sl, in_profit_mult, emergency_pct) = (
            TRAIL_STOP_BACK_PCT,
            TRAIL_STOP_TRIGGER_PCT,
            SCALP_EMERGENCY_PCT,
        );

        // 1. Hard Stops
        let sl_hit = if self.side == PositionSide::Long {
            self.current_price <= self.stop_loss
        } else {
            self.current_price >= self.stop_loss
        };

        if sl_hit {
            return (true, format!("Stop Loss ({:.2}%)", pnl_pct));
        }

        if pnl_pct <= emergency_pct {
            return (true, format!("Emergency ({:.2}%)", pnl_pct));
        }

        // 2. Trailing Stop (High velocity)
        let trigger_price = if self.side == PositionSide::Long {
            self.entry_price * (1.0 + (in_profit_mult / 100.0))
        } else {
            self.entry_price * (1.0 - (in_profit_mult / 100.0))
        };

        let in_profit_zone = if self.side == PositionSide::Long {
            self.highest_price >= trigger_price
        } else {
            self.lowest_price <= trigger_price
        };

        if in_profit_zone {
            let sl_price = if self.side == PositionSide::Long {
                self.highest_price * (1.0 - (trailing_sl / 100.0))
            } else {
                self.lowest_price * (1.0 + (trailing_sl / 100.0))
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
        
        // Emergency exit — position going badly wrong (above SL but accelerating down)
        if pnl_pct <= emergency_pct {
            return (true, "Emergency Exit".to_string());
        }
        
        // ── Time-based exits (DISABLED — Rely on Rebalancing) ────────────────────
        /*
        // Rule: NEVER cut a winning trade on time — let trailing SL handle it.
        // Only free up capital from stale or losing trades.
        if !in_profit {
            // Tier 1 — Stale Trade: price hasn't moved meaningfully after N seconds
            // The momentum thesis is dead. Exit near breakeven before it gets worse.
            if secs_held >= stale_secs && pnl_pct.abs() < stale_thresh {
                return (true, format!("Stale ({}s flat)", secs_held));
            }
            
            // Tier 2 — Time Stop: still underwater after longer window
            // The trade is wrong. Cut it and redeploy capital.
            if secs_held >= time_stop_secs && pnl_pct < 0.0 {
                return (true, format!("Time Stop ({}s)", secs_held));
            }
        }
        
        // Tier 3 — Max Hold: absolute cap, even for profitable trades
        // At this point trailing SL should have already exited winners.
        // If we're still here, something is wrong — exit and reassess.
        if secs_held >= max_hold_secs {
            let label = if in_profit { "Max Hold (profit)" } else { "Max Hold (loss)" };
            return (true, label.to_string());
        }
        */
        
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
        Self {
            total_pnl: 0.0,
            realized_pnl: 0.0,
            trades_count: 0,
            winning_trades: 0,
            losing_trades: 0,
            largest_win: 0.0,
            largest_loss: 0.0,
        }
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
