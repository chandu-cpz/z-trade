use chrono::Local;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;

use crate::config::Config;
use crate::models::{DisplaySignal, Position, PositionSide, Strategy, Trade, TradingStats};
use crate::worker::{SignalMsg, WorkerMsg};

/// In-memory paper trading simulation and risk management engine.
pub struct PaperEngine {
    pub positions: HashMap<String, Position>,
    pub signal_feed: VecDeque<DisplaySignal>,
    pub trades: Vec<Trade>,
    pub stats: TradingStats,
    pub locked_capital: f64,
    pub initial_capital: f64,
    pub circuit_breaker_hit: bool,
    pub debug_mode: bool,
    pub messages: VecDeque<String>,
    pub tick_count: u64,
    pub tick_times: VecDeque<std::time::Instant>,
    pub debug_ticks: VecDeque<String>,
    pub workers: Vec<Sender<WorkerMsg>>,
    pub config: Config,
}

impl PaperEngine {
    pub fn new(config: Config) -> Self {
        let initial_capital = config.initial_capital;
        Self {
            positions: HashMap::new(),
            signal_feed: VecDeque::with_capacity(50),
            trades: Vec::new(),
            stats: TradingStats::new(),
            locked_capital: 0.0,
            initial_capital,
            circuit_breaker_hit: false,
            debug_mode: false,
            messages: VecDeque::with_capacity(20),
            tick_count: 0,
            tick_times: VecDeque::new(),
            debug_ticks: VecDeque::with_capacity(500),
            workers: Vec::new(),
            config,
        }
    }

    pub fn add_message(&mut self, msg: String) {
        let time_str = Local::now().format("%H:%M:%S");
        self.messages.push_back(format!("{} {}", time_str, msg));
        if self.messages.len() > 20 {
            self.messages.pop_front();
        }
    }

    pub fn add_signal(
        &mut self,
        symbol: &str,
        side: PositionSide,
        strategy: Strategy,
        entered: bool,
        reason: Option<&str>,
    ) {
        let sig = DisplaySignal {
            time: Local::now(),
            symbol: symbol.to_string(),
            side,
            strategy,
            entered,
            block_reason: reason.map(|s| s.to_string()),
        };
        self.signal_feed.push_back(sig);
        if self.signal_feed.len() > 50 {
            self.signal_feed.pop_front();
        }
    }

    pub fn free_capital(&self) -> f64 {
        self.initial_capital + self.stats.realized_pnl - self.locked_capital
    }

    pub fn total_value(&self) -> f64 {
        self.free_capital() + self.locked_capital + self.unrealized_pnl()
    }

    pub fn unrealized_pnl(&self) -> f64 {
        self.positions.values().map(|p| p.unrealized_pnl()).sum()
    }

    pub fn total_portfolio_pnl(&self) -> f64 {
        self.stats.realized_pnl + self.unrealized_pnl()
    }

    pub fn can_take_trade(&self) -> bool {
        self.positions.len() < self.config.max_positions
            && !self.circuit_breaker_hit
            && self.free_capital() > self.config.min_capital_per_trade
    }

    pub fn close_trade(&mut self, position: Position, exit_price: f64, reason: &str) {
        let pnl = match position.side {
            PositionSide::Long => (exit_price - position.entry_price) * position.quantity as f64,
            PositionSide::Short => (position.entry_price - exit_price) * position.quantity as f64,
        };

        // Release locked margin
        let locked = position.entry_price * position.quantity as f64;
        self.locked_capital = (self.locked_capital - locked).max(0.0);

        // Record stats
        self.stats.record_trade(pnl);

        // Circuit breaker check
        if self.total_portfolio_pnl() <= -self.config.max_loss {
            self.circuit_breaker_hit = true;
            self.add_message(format!(
                "⚠ CIRCUIT BREAKER TRIGGERED at -₹{:.2} loss!",
                -self.total_portfolio_pnl()
            ));
        }

        self.add_message(format!(
            "EXIT {} @ {:.2} | P&L: ₹{:.2} | {}",
            position.symbol, exit_price, pnl, reason
        ));

        self.trades.push(Trade {
            symbol: position.symbol,
            side: position.side,
            strategy: position.strategy,
            entry_price: position.entry_price,
            exit_price,
            quantity: position.quantity,
            pnl,
            entry_time: position.entry_time,
            exit_time: Local::now(),
            exit_reason: reason.to_string(),
        });
    }

    pub fn calculate_position_size(&self, price: f64, uas: f64) -> u32 {
        let mut max_value = self.free_capital().min(5000.0);

        // SWS: Signal-Weighted Sizing
        if uas > 90.0 {
            max_value *= 1.5;
        } else if uas > 80.0 {
            max_value *= 1.25;
        }

        if price <= 0.0 || price > self.config.max_price {
            return 0;
        }

        let shares = (max_value / price) as u32;
        shares.clamp(1, 100)
    }

    pub fn handle_signal(&mut self, msg: SignalMsg) {
        let sym = &msg.symbol;
        let now_time = Local::now();

        // 1. Loss Cooldown Check
        let cooldown_ok = {
            let last_loss = self
                .trades
                .iter()
                .rev()
                .find(|t| t.symbol == *sym && t.pnl < 0.0)
                .map(|t| t.exit_time);

            if let Some(t) = last_loss {
                now_time.signed_duration_since(t).num_seconds() >= self.config.loss_cooldown_secs
            } else {
                true
            }
        };

        if !cooldown_ok {
            self.add_signal(sym, msg.side, msg.strategy, false, Some("loss cooldown"));
            return;
        }

        if self.positions.contains_key(sym) {
            return;
        }

        // 2. Dynamic Hurdle Logic
        let utilization = self.positions.len() as f64 / self.config.max_positions as f64;
        let dynamic_threshold = self.config.ies_base_threshold
            + (utilization * (self.config.ies_max_threshold - self.config.ies_base_threshold));

        if msg.uas < dynamic_threshold {
            self.add_signal(
                sym,
                msg.side,
                msg.strategy,
                false,
                Some(&format!("IES < {:.0}", dynamic_threshold)),
            );
            return;
        }

        let mut proceed = false;
        let can_afford = self.free_capital() >= msg.price;

        if self.can_take_trade() && can_afford {
            proceed = true;
        } else {
            // 3. Velocity Scalper Rotation: Find the lowest alpha_score trade
            let victim = self
                .positions
                .iter()
                .map(|(k, p)| (k.clone(), p.alpha_score()))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((lazy_sym, old_score)) = victim {
                if msg.uas > (old_score + self.config.ies_swap_advantage) {
                    if let Some(pos_removed) = self.positions.remove(&lazy_sym) {
                        let exit_price = pos_removed.current_price;
                        self.close_trade(pos_removed, exit_price, "VelocitySwap");
                        self.add_message(format!(
                            "SWAP: Rotating {} (Score:{:.0}) -> {} (Score:{:.0})",
                            lazy_sym, old_score, sym, msg.uas
                        ));
                        proceed = true;
                    }
                }
            }
        }

        if proceed {
            let qty = self.calculate_position_size(msg.price, msg.uas);
            if qty > 0 {
                let position = Position::new(
                    sym.clone(),
                    msg.side,
                    msg.strategy,
                    msg.price,
                    qty,
                    &self.config,
                );
                self.locked_capital += msg.price * (qty as f64);
                self.add_message(format!(
                    "ENTER {} {} {} @{:.2} x{}",
                    sym, msg.side, msg.strategy, msg.price, qty
                ));
                self.add_signal(sym, msg.side, msg.strategy, true, None);
                self.positions.insert(sym.clone(), position);
            } else {
                self.add_signal(sym, msg.side, msg.strategy, false, Some("price > limit"));
            }
        }
    }
}
