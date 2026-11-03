#![allow(dead_code)]

use chrono::{DateTime, Local, Timelike};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender};

use crate::config::Config;
use crate::indicators::{calculate_order_book_imbalance, calculate_std_dev, RsiState};
use crate::models::{PositionSide, PricePoint, Strategy};
use crate::strategy::{
    calculate_ies, detect_depth_pressure, detect_liquidity_sweep, detect_mean_reversion,
    detect_opening_sniper, detect_scalp_momentum, StrategyContext,
};

#[derive(Debug, Clone)]
pub enum WorkerMsg {
    Tick {
        token: String,
        price: f64,
        volume: u64,
        total_buy_qty: u64,
        total_sell_qty: u64,
        best_bid: f64,
        best_ask: f64,
        last_trade_qty: u64,
        timestamp: DateTime<Local>,
    },
    Terminate,
}

#[derive(Debug, Clone)]
pub struct SignalMsg {
    pub symbol: String,
    pub price: f64,
    pub side: PositionSide,
    pub strategy: Strategy,
    pub timestamp: DateTime<Local>,
    pub uas: f64,
}

pub struct StockTracker {
    pub symbol: String,
    pub price_history: VecDeque<PricePoint>,
    pub volume_history: VecDeque<u64>,
    pub current_price: f64,
    pub last_alert: Option<DateTime<Local>>,
    pub pending_signal: Option<(PositionSide, Strategy)>,
    pub confirmation_count: usize,

    // Order flow
    pub total_buy_qty: u64,
    pub total_sell_qty: u64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub last_trade_qty: u64,
    pub ltq_history: VecDeque<u64>,

    // Context & trend
    pub trend_direction: i8,
    pub tick_buf: VecDeque<f64>,

    // Indicators
    pub cumulative_pv: f64,
    pub cumulative_vol: u64,
    pub vwap: f64,
    pub imbalance: f64,
    pub rsi_state: RsiState,
    pub ema_short: f64,
    pub ema_long: f64,
    pub std_dev: f64,

    // Opening range
    pub opening_high: f64,
    pub opening_low: f64,
}

impl StockTracker {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
            price_history: VecDeque::with_capacity(300),
            volume_history: VecDeque::with_capacity(120),
            current_price: 0.0,
            last_alert: None,
            pending_signal: None,
            confirmation_count: 0,
            total_buy_qty: 0,
            total_sell_qty: 0,
            best_bid: 0.0,
            best_ask: 0.0,
            last_trade_qty: 0,
            ltq_history: VecDeque::with_capacity(60),
            trend_direction: 0,
            tick_buf: VecDeque::with_capacity(60),
            cumulative_pv: 0.0,
            cumulative_vol: 0,
            vwap: 0.0,
            imbalance: 0.0,
            rsi_state: RsiState::default(),
            ema_short: 0.0,
            ema_long: 0.0,
            std_dev: 0.0,
            opening_high: 0.0,
            opening_low: 0.0,
        }
    }

    pub fn update(
        &mut self,
        price: f64,
        volume: u64,
        tbq: u64,
        tsq: u64,
        bp: f64,
        sp: f64,
        ltq: u64,
        now: DateTime<Local>,
        config: &Config,
    ) {
        self.current_price = price;

        // VWAP
        if volume > 0 {
            self.cumulative_pv += price * (volume as f64);
            self.cumulative_vol += volume;
            if self.cumulative_vol > 0 {
                self.vwap = self.cumulative_pv / (self.cumulative_vol as f64);
            }
        }

        // Order book
        if tbq > 0 {
            self.total_buy_qty = tbq;
        }
        if tsq > 0 {
            self.total_sell_qty = tsq;
        }
        self.imbalance = calculate_order_book_imbalance(self.total_buy_qty, self.total_sell_qty);

        if bp > 0.0 {
            self.best_bid = bp;
        }
        if sp > 0.0 {
            self.best_ask = sp;
        }
        if ltq > 0 {
            self.last_trade_qty = ltq;
            self.ltq_history.push_back(ltq);
            if self.ltq_history.len() > 60 {
                self.ltq_history.pop_front();
            }
        }

        self.price_history.push_back(PricePoint { price, timestamp: now });
        self.volume_history.push_back(volume);

        // Limit price history window
        if self.price_history.len() > 3600 {
            while let Some(oldest) = self.price_history.front() {
                if now.signed_duration_since(oldest.timestamp).num_seconds() > config.trend_window_secs + 60 {
                    self.price_history.pop_front();
                } else {
                    break;
                }
            }
        }
        if self.volume_history.len() > 120 {
            self.volume_history.pop_front();
        }

        // Trend calculation
        self.compute_trend(now, config.trend_window_secs);

        // Tick buffer & indicator updates
        let prev_price = self.tick_buf.back().cloned();
        if prev_price.map(|last| (last - price).abs() > 0.0001).unwrap_or(true) {
            self.tick_buf.push_back(price);

            if let Some(prev) = prev_price {
                self.rsi_state.update(prev, price);

                // EMAs
                if self.ema_short == 0.0 {
                    self.ema_short = price;
                }
                if self.ema_long == 0.0 {
                    self.ema_long = price;
                }
                let alpha_short = 2.0 / (13.0 + 1.0);
                let alpha_long = 2.0 / (50.0 + 1.0);
                self.ema_short = (price * alpha_short) + (self.ema_short * (1.0 - alpha_short));
                self.ema_long = (price * alpha_long) + (self.ema_long * (1.0 - alpha_long));

                // Std Dev
                self.std_dev = calculate_std_dev(&self.tick_buf);
            }

            if self.tick_buf.len() > config.min_ticks {
                self.tick_buf.pop_front();
            }
        }

        // Opening range tracking (09:15 - 09:20)
        let hour = now.hour();
        let min = now.minute();
        if hour == config.session_start_hour
            && min >= config.session_start_min
            && min < config.session_start_min + (config.orb_window_mins as u32)
        {
            if self.opening_high == 0.0 || price > self.opening_high {
                self.opening_high = price;
            }
            if self.opening_low == 0.0 || price < self.opening_low {
                self.opening_low = price;
            }
        }
    }

    fn compute_trend(&mut self, now: DateTime<Local>, trend_window_secs: i64) {
        if self.price_history.is_empty() {
            return;
        }
        let trend_start = now - chrono::Duration::seconds(trend_window_secs);
        if let Some(old) = self.price_history.iter().find(|pp| pp.timestamp >= trend_start) {
            if old.price > 0.0 {
                let change = (self.current_price - old.price) / old.price * 100.0;
                self.trend_direction = if change > 0.10 {
                    1
                } else if change < -0.10 {
                    -1
                } else {
                    0
                };
            }
        }
    }

    pub fn avg_volume(&self) -> f64 {
        if self.volume_history.is_empty() {
            return 0.0;
        }
        let sum: u64 = self.volume_history.iter().sum();
        sum as f64 / self.volume_history.len() as f64
    }

    pub fn avg_ltq(&self) -> f64 {
        if self.ltq_history.is_empty() {
            return 0.0;
        }
        self.ltq_history.iter().sum::<u64>() as f64 / self.ltq_history.len() as f64
    }

    pub fn build_context<'a>(&'a self, volume: u64) -> StrategyContext<'a> {
        StrategyContext {
            current_price: self.current_price,
            volume,
            avg_volume: self.avg_volume(),
            last_trade_qty: self.last_trade_qty,
            avg_ltq: self.avg_ltq(),
            total_buy_qty: self.total_buy_qty,
            total_sell_qty: self.total_sell_qty,
            best_bid: self.best_bid,
            best_ask: self.best_ask,
            vwap: self.vwap,
            rsi: self.rsi_state.rsi,
            ema_short: self.ema_short,
            std_dev: self.std_dev,
            imbalance: self.imbalance,
            trend_direction: self.trend_direction,
            opening_high: self.opening_high,
            opening_low: self.opening_low,
            tick_buf: &self.tick_buf,
            price_history: &self.price_history,
        }
    }
}

pub struct Worker {
    pub id: usize,
    pub trackers: HashMap<String, StockTracker>,
    pub signal_tx: Sender<SignalMsg>,
    pub config: Config,
}

impl Worker {
    pub fn new(id: usize, signal_tx: Sender<SignalMsg>, config: Config) -> Self {
        Self {
            id,
            trackers: HashMap::new(),
            signal_tx,
            config,
        }
    }

    pub fn run(mut self, rx: Receiver<WorkerMsg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                WorkerMsg::Terminate => break,
                WorkerMsg::Tick {
                    token,
                    price,
                    volume,
                    total_buy_qty,
                    total_sell_qty,
                    best_bid,
                    best_ask,
                    last_trade_qty,
                    timestamp,
                } => {
                    let config = &self.config;
                    let tracker = self
                        .trackers
                        .entry(token.clone())
                        .or_insert_with(|| StockTracker::new(token.clone()));

                    tracker.update(
                        price,
                        volume,
                        total_buy_qty,
                        total_sell_qty,
                        best_bid,
                        best_ask,
                        last_trade_qty,
                        timestamp,
                        config,
                    );

                    // Check confirmation if signal is pending
                    if let Some((pending_side, pending_strategy)) = tracker.pending_signal {
                        tracker.confirmation_count += 1;
                        if tracker.confirmation_count >= config.confirmation_ticks {
                            let spread_ok = if tracker.best_bid > 0.0 && tracker.best_ask > 0.0 {
                                ((tracker.best_ask - tracker.best_bid) / tracker.best_bid) * 100.0
                                    <= config.max_spread_pct
                            } else {
                                true
                            };

                            let session_ok = timestamp.hour() > config.session_start_hour
                                || (timestamp.hour() == config.session_start_hour
                                    && timestamp.minute() >= config.session_start_min);

                            if spread_ok && session_ok {
                                let final_side = if config.invert_strategy {
                                    match pending_side {
                                        PositionSide::Long => PositionSide::Short,
                                        PositionSide::Short => PositionSide::Long,
                                    }
                                } else {
                                    pending_side
                                };

                                let ctx = tracker.build_context(volume);
                                let uas = calculate_ies(&ctx, config);

                                let _ = self.signal_tx.send(SignalMsg {
                                    symbol: token.clone(),
                                    price: tracker.current_price,
                                    side: final_side,
                                    strategy: pending_strategy,
                                    timestamp,
                                    uas,
                                });

                                tracker.last_alert = Some(timestamp);
                            }

                            tracker.pending_signal = None;
                            tracker.confirmation_count = 0;
                        } else if tracker.confirmation_count > 10 {
                            tracker.pending_signal = None;
                            tracker.confirmation_count = 0;
                        }
                    } else {
                        // Scan for new signal
                        let in_cooldown = if let Some(last) = tracker.last_alert {
                            timestamp.signed_duration_since(last).num_seconds()
                                < config.alert_cooldown_secs
                        } else {
                            false
                        };

                        if !in_cooldown {
                            let ctx = tracker.build_context(volume);

                            // Strategy Priority Ranking:
                            // 1. Opening Sniper (ORB)
                            let detected = detect_opening_sniper(&ctx, timestamp, config)
                                // 2. Mean Reversion
                                .or_else(|| detect_mean_reversion(&ctx, config))
                                // 3. Liquidity Sweep
                                .or_else(|| detect_liquidity_sweep(&ctx, config))
                                // 4. Depth Pressure (OFI)
                                .or_else(|| detect_depth_pressure(&ctx, config))
                                // 5. Momentum Follow
                                .or_else(|| detect_scalp_momentum(&ctx, config));

                            if let Some(sig) = detected {
                                tracker.pending_signal = Some(sig);
                                tracker.confirmation_count = 0;
                            }
                        }
                    }
                }
            }
        }
    }
}
