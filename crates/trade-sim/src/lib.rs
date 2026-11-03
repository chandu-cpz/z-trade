use chrono::Local;
use std::collections::{HashMap, VecDeque};

use trade_core::config::Config;
use trade_core::position::Position;
use trade_core::signal::{DisplaySignal, Signal};
use trade_core::store::{PendingOrder, SessionState};
use trade_core::trade::{Trade, TradingStats};
use trade_core::types::{PositionSide, StrategyType};

/// High-fidelity paper trading simulation ledger with realistic slippage and fee accounting.
pub struct PaperSimulationEngine {
    pub positions: HashMap<String, Position>,
    pub pending_orders: Vec<PendingOrder>,
    pub signal_feed: VecDeque<DisplaySignal>,
    pub trades: Vec<Trade>,
    pub stats: TradingStats,
    pub locked_capital: f64,
    pub initial_capital: f64,
    pub circuit_breaker_hit: bool,
    pub messages: VecDeque<String>,
    pub watchlist: Vec<String>,
    pub algo_enabled: bool,
    pub session_date: String,
    pub tick_count: u64,
    pub tick_times: VecDeque<std::time::Instant>,
    pub debug_mode: bool,
    pub debug_ticks: VecDeque<String>,
    pub config: Config,
}

impl PaperSimulationEngine {
    pub fn new(config: Config) -> Self {
        let initial_capital = config.portfolio.initial_capital;
        let today = Local::now().format("%Y-%m-%d").to_string();
        Self {
            positions: HashMap::new(),
            pending_orders: Vec::new(),
            signal_feed: VecDeque::with_capacity(100),
            trades: Vec::new(),
            stats: TradingStats::new(),
            locked_capital: 0.0,
            initial_capital,
            circuit_breaker_hit: false,
            messages: VecDeque::with_capacity(50),
            watchlist: vec![
                "RELIANCE-EQ".to_string(),
                "TCS-EQ".to_string(),
                "HDFCBANK-EQ".to_string(),
                "INFY-EQ".to_string(),
                "ICICIBANK-EQ".to_string(),
                "SBIN-EQ".to_string(),
            ],
            algo_enabled: true,
            session_date: today,
            tick_count: 0,
            tick_times: VecDeque::with_capacity(500),
            debug_mode: false,
            debug_ticks: VecDeque::with_capacity(500),
            config,
        }
    }

    /// Rehydrates the engine from persisted session state for across-the-day crash recovery.
    pub fn from_session(config: Config, session: SessionState) -> Self {
        let mut engine = Self::new(config);
        engine.initial_capital = session.initial_capital;
        engine.locked_capital = session.locked_capital;
        engine.positions = session.positions;
        engine.pending_orders = session.pending_orders;
        engine.trades = session.trades;
        engine.stats = session.stats;
        engine.watchlist = session.watchlist;
        engine.algo_enabled = session.algo_enabled;
        engine.session_date = session.session_date;
        engine.log_message(format!(
            "Restored active session from disk: {} open holdings, {} trades.",
            engine.positions.len(),
            engine.trades.len()
        ));
        engine
    }

    /// Exports current memory state into a serializable SessionState.
    pub fn to_session(&self) -> SessionState {
        SessionState {
            session_date: self.session_date.clone(),
            initial_capital: self.initial_capital,
            free_capital: self.free_capital(),
            locked_capital: self.locked_capital,
            positions: self.positions.clone(),
            pending_orders: self.pending_orders.clone(),
            trades: self.trades.clone(),
            stats: self.stats.clone(),
            watchlist: self.watchlist.clone(),
            algo_enabled: self.algo_enabled,
            last_saved_at: Local::now(),
        }
    }

    pub fn log_message(&mut self, msg: String) {
        let time_str = Local::now().format("%H:%M:%S");
        self.messages.push_back(format!("{} {}", time_str, msg));
        if self.messages.len() > 100 {
            self.messages.pop_front();
        }
    }

    pub fn record_signal(
        &mut self,
        sig: &Signal,
        entered: bool,
        block_reason: Option<&str>,
    ) {
        let item = DisplaySignal {
            time: sig.timestamp,
            symbol: sig.symbol.clone(),
            side: sig.side,
            strategy: sig.strategy,
            entered,
            block_reason: block_reason.map(|s| s.to_string()),
        };
        self.signal_feed.push_back(item);
        if self.signal_feed.len() > 100 {
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
        self.positions.len() < self.config.portfolio.max_positions
            && !self.circuit_breaker_hit
            && self.free_capital() > self.config.portfolio.min_capital_per_trade
    }

    pub fn calculate_fees(&self, entry_turnover: f64, exit_turnover: f64) -> f64 {
        let cfg = &self.config.transaction_costs;
        let brokerage = cfg.simulated_brokerage_per_order * 2.0;
        let stt = exit_turnover * (cfg.simulated_stt_pct / 100.0);
        let turnover_fee = (entry_turnover + exit_turnover) * (cfg.simulated_turnover_charges_pct / 100.0);
        let gst = (brokerage + turnover_fee) * 0.18;
        brokerage + stt + turnover_fee + gst
    }

    pub fn apply_slippage(&self, raw_price: f64, side: PositionSide, is_entry: bool) -> f64 {
        let slip_pct = self.config.transaction_costs.simulated_slippage_pct / 100.0;
        match (side, is_entry) {
            (PositionSide::Long, true) | (PositionSide::Short, false) => raw_price * (1.0 + slip_pct),
            (PositionSide::Long, false) | (PositionSide::Short, true) => raw_price * (1.0 - slip_pct),
        }
    }

    pub fn close_trade(&mut self, position: Position, raw_exit_price: f64, reason: &str) {
        let exit_price = self.apply_slippage(raw_exit_price, position.side, false);
        let entry_turnover = position.entry_price * (position.quantity as f64);
        let exit_turnover = exit_price * (position.quantity as f64);

        let gross_pnl = match position.side {
            PositionSide::Long => exit_turnover - entry_turnover,
            PositionSide::Short => entry_turnover - exit_turnover,
        };

        let fees = self.calculate_fees(entry_turnover, exit_turnover);
        let net_pnl = gross_pnl - fees;

        self.locked_capital = (self.locked_capital - entry_turnover).max(0.0);
        self.stats.record_trade(net_pnl, gross_pnl, fees);

        if self.total_portfolio_pnl() <= -self.config.portfolio.max_loss {
            self.circuit_breaker_hit = true;
            self.log_message(format!(
                "⚠ CIRCUIT BREAKER TRIGGERED at -₹{:.2} loss!",
                -self.total_portfolio_pnl()
            ));
        }

        self.log_message(format!(
            "EXIT {} @ {:.2} | Net P&L: ₹{:.2} (Fees: ₹{:.2}) | {}",
            position.symbol, exit_price, net_pnl, fees, reason
        ));

        self.trades.push(Trade {
            symbol: position.symbol,
            token: position.token,
            side: position.side,
            strategy: position.strategy,
            entry_price: position.entry_price,
            exit_price,
            quantity: position.quantity,
            gross_pnl,
            transaction_costs: fees,
            net_pnl,
            entry_time: position.entry_time,
            exit_time: Local::now(),
            exit_reason: reason.to_string(),
        });
    }

    pub fn calculate_position_size(&self, price: f64, uas: f64) -> u32 {
        let mut max_value = self.free_capital().min(5000.0);

        if uas > 90.0 {
            max_value *= 1.5;
        } else if uas > 80.0 {
            max_value *= 1.25;
        }

        if price <= 0.0 || price > self.config.portfolio.max_price {
            return 0;
        }

        let shares = (max_value / price) as u32;
        shares.clamp(1, 100)
    }

    pub fn handle_signal(&mut self, sig: Signal) {
        let sym = &sig.symbol;
        let now_time = Local::now();

        // 1. Loss Cooldown Check
        let cooldown_ok = {
            let last_loss = self
                .trades
                .iter()
                .rev()
                .find(|t| t.symbol == *sym && t.net_pnl < 0.0)
                .map(|t| t.exit_time);

            if let Some(t) = last_loss {
                now_time.signed_duration_since(t).num_seconds() >= self.config.filters.loss_cooldown_secs
            } else {
                true
            }
        };

        if !cooldown_ok {
            self.record_signal(&sig, false, Some("loss cooldown"));
            return;
        }

        if self.positions.contains_key(sym) {
            return;
        }

        // 2. Dynamic IES Hurdle Logic
        let utilization = self.positions.len() as f64 / self.config.portfolio.max_positions as f64;
        let dynamic_threshold = self.config.impulse_scoring.ies_base_threshold
            + (utilization * (self.config.impulse_scoring.ies_max_threshold - self.config.impulse_scoring.ies_base_threshold));

        if sig.alpha_score < dynamic_threshold {
            self.record_signal(
                &sig,
                false,
                Some(&format!("IES < {:.0}", dynamic_threshold)),
            );
            return;
        }

        let mut proceed = false;
        let can_afford = self.free_capital() >= sig.price;

        if self.can_take_trade() && can_afford {
            proceed = true;
        } else {
            // 3. Velocity Swap Rotation
            let victim = self
                .positions
                .iter()
                .map(|(k, p)| (k.clone(), p.alpha_score()))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((lazy_sym, old_score)) = victim {
                if sig.alpha_score > (old_score + self.config.impulse_scoring.ies_swap_advantage) {
                    if let Some(pos_removed) = self.positions.remove(&lazy_sym) {
                        let exit_price = pos_removed.current_price;
                        self.close_trade(pos_removed, exit_price, "VelocitySwap");
                        self.log_message(format!(
                            "SWAP: Rotating {} (Score:{:.0}) -> {} (Score:{:.0})",
                            lazy_sym, old_score, sym, sig.alpha_score
                        ));
                        proceed = true;
                    }
                }
            }
        }

        if proceed {
            let qty = self.calculate_position_size(sig.price, sig.alpha_score);
            if qty > 0 {
                let entry_price = self.apply_slippage(sig.price, sig.side, true);
                let position = Position::new(
                    sym.clone(),
                    sig.token,
                    sig.side,
                    sig.strategy,
                    entry_price,
                    qty,
                    &self.config,
                );
                self.locked_capital += entry_price * (qty as f64);
                self.log_message(format!(
                    "ENTER {} {} {} @{:.2} x{}",
                    sym, sig.side, sig.strategy, entry_price, qty
                ));
                self.record_signal(&sig, true, None);
                self.positions.insert(sym.clone(), position);
            } else {
                self.record_signal(&sig, false, Some("price > limit"));
            }
        }
    }

    /// Evaluates open positions on each market tick for trailing stops, profit targets, or SL hits.
    pub fn update_tick(&mut self, tick: &trade_core::tick::Tick) {
        self.tick_count += 1;
        let now = std::time::Instant::now();
        self.tick_times.push_back(now);
        while let Some(front) = self.tick_times.front() {
            if now.duration_since(*front) > std::time::Duration::from_secs(3) {
                self.tick_times.pop_front();
            } else {
                break;
            }
        }

        if self.debug_mode {
            self.debug_ticks.push_back(format!(
                "{} {} {:.2} v:{}",
                tick.timestamp.format("%H:%M:%S%.3f"),
                tick.symbol,
                tick.price,
                tick.volume
            ));
            if self.debug_ticks.len() > 500 {
                self.debug_ticks.pop_front();
            }
        }

        // Evaluate and fill working pending orders on tick price cross
        let mut filled_indices = Vec::new();
        for (i, order) in self.pending_orders.iter().enumerate() {
            if order.symbol != tick.symbol {
                continue;
            }
            let fill = match (order.side, order.order_type.as_str()) {
                (PositionSide::Long, "LIMIT") => {
                    order.limit_price.map(|lp| tick.price <= lp).unwrap_or(false)
                }
                (PositionSide::Short, "LIMIT") => {
                    order.limit_price.map(|lp| tick.price >= lp).unwrap_or(false)
                }
                (PositionSide::Long, "STOP_LOSS") => {
                    order.trigger_price.map(|tp| tick.price >= tp).unwrap_or(false)
                }
                (PositionSide::Short, "STOP_LOSS") => {
                    order.trigger_price.map(|tp| tick.price <= tp).unwrap_or(false)
                }
                _ => false,
            };
            if fill {
                filled_indices.push(i);
            }
        }

        // Process filled orders in reverse order
        for idx in filled_indices.into_iter().rev() {
            let order = self.pending_orders.remove(idx);
            let entry_price = self.apply_slippage(tick.price, order.side, true);
            let mut pos = Position::new(
                order.symbol.clone(),
                order.token,
                order.side,
                StrategyType::Manual,
                entry_price,
                order.quantity,
                &self.config,
            );
            if let Some(sl) = order.stop_loss {
                pos.stop_loss = sl;
            }
            if let Some(tgt) = order.target {
                pos.target = tgt;
            }
            let cost = entry_price * (order.quantity as f64);
            self.locked_capital += cost;
            self.log_message(format!(
                "ORDER FILLED: {} {} @{:.2} x{} (ID: {})",
                order.side, order.symbol, entry_price, order.quantity, order.id
            ));
            self.positions.insert(order.symbol, pos);
        }

        let mut exit_reason = None;
        if let Some(pos) = self.positions.get_mut(&tick.symbol) {
            pos.update_price(tick.price);
            let (exit, reason) = pos.should_exit(&self.config);
            if exit {
                exit_reason = Some(reason);
            }
        }
        if let Some(reason) = exit_reason {
            if let Some(pos) = self.positions.remove(&tick.symbol) {
                self.close_trade(pos, tick.price, &reason);
            }
        }
    }

    /// Submit a new manual or algorithmic order into the OMS
    pub fn submit_order(&mut self, order: PendingOrder, current_market_price: f64) {
        if order.order_type == "MARKET" {
            let ref_price = if current_market_price > 0.0 { current_market_price } else { 100.0 };
            let entry_price = self.apply_slippage(ref_price, order.side, true);
            let mut pos = Position::new(
                order.symbol.clone(),
                order.token,
                order.side,
                StrategyType::Manual,
                entry_price,
                order.quantity,
                &self.config,
            );
            if let Some(sl) = order.stop_loss {
                pos.stop_loss = sl;
            }
            if let Some(tgt) = order.target {
                pos.target = tgt;
            }
            let cost = entry_price * (order.quantity as f64);
            self.locked_capital += cost;
            self.log_message(format!(
                "MANUAL MARKET ORDER: {} {} @{:.2} x{}",
                order.side, order.symbol, entry_price, order.quantity
            ));
            self.positions.insert(order.symbol, pos);
        } else {
            self.log_message(format!(
                "PENDING ORDER SUBMITTED: {} {} {} x{} (ID: {})",
                order.order_type, order.side, order.symbol, order.quantity, order.id
            ));
            self.pending_orders.push(order);
        }
    }

    /// Cancel a working pending order
    pub fn cancel_order(&mut self, order_id: &str) -> bool {
        if let Some(pos) = self.pending_orders.iter().position(|o| o.id == order_id) {
            let removed = self.pending_orders.remove(pos);
            self.log_message(format!("ORDER CANCELLED: {} {}", removed.id, removed.symbol));
            true
        } else {
            false
        }
    }

    /// Cancel all pending orders
    pub fn cancel_all_pending(&mut self) -> usize {
        let count = self.pending_orders.len();
        self.pending_orders.clear();
        if count > 0 {
            self.log_message(format!("CANCELLED ALL {} PENDING ORDERS", count));
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use trade_core::config::Config;
    use trade_core::store::PendingOrder;
    use trade_core::tick::Tick;
    use trade_core::types::PositionSide;

    #[test]
    fn test_market_order_submission() {
        let config = Config::default();
        let mut sim = PaperSimulationEngine::new(config);

        let order = PendingOrder {
            id: "MKT-1".to_string(),
            symbol: "RELIANCE-EQ".to_string(),
            token: 2885,
            side: PositionSide::Long,
            order_type: "MARKET".to_string(),
            quantity: 10,
            limit_price: None,
            trigger_price: None,
            stop_loss: Some(2400.0),
            target: Some(2600.0),
            placed_at: Local::now(),
            origin: "MANUAL".to_string(),
        };

        sim.submit_order(order, 2500.0);
        assert_eq!(sim.positions.len(), 1);
        assert!(sim.positions.contains_key("RELIANCE-EQ"));
        assert!(sim.pending_orders.is_empty());
    }

    #[test]
    fn test_limit_order_execution_on_tick() {
        let config = Config::default();
        let mut sim = PaperSimulationEngine::new(config);

        let order = PendingOrder {
            id: "LMT-1".to_string(),
            symbol: "INFY-EQ".to_string(),
            token: 1594,
            side: PositionSide::Long,
            order_type: "LIMIT".to_string(),
            quantity: 5,
            limit_price: Some(1500.0),
            trigger_price: None,
            stop_loss: Some(1480.0),
            target: Some(1550.0),
            placed_at: Local::now(),
            origin: "MANUAL".to_string(),
        };

        // Submit limit order @ 1500.0
        sim.submit_order(order, 1510.0);
        assert_eq!(sim.pending_orders.len(), 1);
        assert_eq!(sim.positions.len(), 0);

        // Tick at 1505 (above limit for Long) -> should not fill
        let tick1 = Tick {
            symbol: "INFY-EQ".to_string(),
            token: 1594,
            price: 1505.0,
            volume: 100,
            last_trade_qty: 10,
            best_bid: 1504.9,
            best_ask: 1505.0,
            total_buy_qty: 1000,
            total_sell_qty: 1000,
            timestamp: Local::now(),
        };
        sim.update_tick(&tick1);
        assert_eq!(sim.pending_orders.len(), 1);
        assert_eq!(sim.positions.len(), 0);

        // Tick at 1499.0 (drops below limit) -> should fill!
        let tick2 = Tick {
            symbol: "INFY-EQ".to_string(),
            token: 1594,
            price: 1499.0,
            volume: 120,
            last_trade_qty: 20,
            best_bid: 1498.9,
            best_ask: 1499.0,
            total_buy_qty: 1000,
            total_sell_qty: 1000,
            timestamp: Local::now(),
        };
        sim.update_tick(&tick2);
        assert_eq!(sim.pending_orders.len(), 0);
        assert_eq!(sim.positions.len(), 1);
        assert!(sim.positions.contains_key("INFY-EQ"));
    }
}


