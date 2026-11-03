use std::collections::{HashMap, VecDeque};
use chrono::{DateTime, Local, Timelike};

use trade_core::config::Config;
use trade_core::signal::Signal;
use trade_core::tick::Tick;
use trade_core::types::{PositionSide, StrategyType};
use trade_indicators::{BarAggregator, Ema, OhlcvBar, RollingStdDev, Rsi, Vwap};

use crate::{
    calculate_ies, DepthPressure, LiquiditySweep, MeanReversion, MomentumScalp, OpeningSniper,
    Strategy, StrategyContext,
};

pub struct StockTracker {
    pub symbol: String,
    pub token: u32,
    pub current_price: f64,
    pub vwap: Vwap,
    pub rsi: Rsi,
    pub ema_short: Ema,
    pub ema_long: Ema,
    pub rolling_std: RollingStdDev,

    pub total_buy_qty: u64,
    pub total_sell_qty: u64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub last_trade_qty: u64,

    pub ltq_history: VecDeque<u64>,
    pub volume_history: VecDeque<u64>,
    pub price_history: VecDeque<(f64, DateTime<Local>)>,
    pub tick_buf: VecDeque<f64>,

    pub trend_direction: i8,
    pub opening_high: f64,
    pub opening_low: f64,

    pub pending_signal: Option<(PositionSide, StrategyType)>,
    pub confirmation_count: usize,
    pub last_alert: Option<DateTime<Local>>,
    pub bar_aggregator: BarAggregator,
}

impl StockTracker {
    pub fn new(symbol: String, token: u32) -> Self {
        Self {
            symbol,
            token,
            current_price: 0.0,
            vwap: Vwap::new(),
            rsi: Rsi::new(14),
            ema_short: Ema::new(13),
            ema_long: Ema::new(50),
            rolling_std: RollingStdDev::new(20),
            total_buy_qty: 0,
            total_sell_qty: 0,
            best_bid: 0.0,
            best_ask: 0.0,
            last_trade_qty: 0,
            ltq_history: VecDeque::with_capacity(60),
            volume_history: VecDeque::with_capacity(120),
            price_history: VecDeque::with_capacity(3600),
            tick_buf: VecDeque::with_capacity(60),
            trend_direction: 0,
            opening_high: 0.0,
            opening_low: 0.0,
            pending_signal: None,
            confirmation_count: 0,
            last_alert: None,
            bar_aggregator: BarAggregator::new(60, 120),
        }
    }

    pub fn avg_volume(&self) -> f64 {
        if self.volume_history.is_empty() {
            0.0
        } else {
            let sum: u64 = self.volume_history.iter().sum();
            sum as f64 / self.volume_history.len() as f64
        }
    }

    pub fn avg_ltq(&self) -> f64 {
        if self.ltq_history.is_empty() {
            0.0
        } else {
            let sum: u64 = self.ltq_history.iter().sum();
            sum as f64 / self.ltq_history.len() as f64
        }
    }

    fn compute_trend(&mut self, now: DateTime<Local>, trend_window_secs: i64) {
        if self.price_history.is_empty() {
            return;
        }
        let trend_start = now - chrono::Duration::seconds(trend_window_secs);
        if let Some(&(price, _)) = self.price_history.iter().find(|(_, ts)| *ts >= trend_start) {
            if price > 0.0 {
                let change = (self.current_price - price) / price * 100.0;
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

    pub fn on_tick(&mut self, tick: &Tick, config: &Config) -> Option<Signal> {
        let price = tick.price;
        self.current_price = price;
        self.vwap.update(price, tick.volume);
        self.bar_aggregator.update(price, tick.volume, tick.timestamp);

        if tick.total_buy_qty > 0 {
            self.total_buy_qty = tick.total_buy_qty;
        }
        if tick.total_sell_qty > 0 {
            self.total_sell_qty = tick.total_sell_qty;
        }
        if tick.best_bid > 0.0 {
            self.best_bid = tick.best_bid;
        }
        if tick.best_ask > 0.0 {
            self.best_ask = tick.best_ask;
        }
        if tick.last_trade_qty > 0 {
            self.last_trade_qty = tick.last_trade_qty;
            self.ltq_history.push_back(tick.last_trade_qty);
            if self.ltq_history.len() > 60 {
                self.ltq_history.pop_front();
            }
        }

        self.price_history.push_back((price, tick.timestamp));
        self.volume_history.push_back(tick.volume);

        if self.price_history.len() > 3600 {
            let cutoff = tick.timestamp - chrono::Duration::seconds(config.filters.trend_window_secs + 60);
            while let Some(&(_, ts)) = self.price_history.front() {
                if ts < cutoff {
                    self.price_history.pop_front();
                } else {
                    break;
                }
            }
        }
        if self.volume_history.len() > 120 {
            self.volume_history.pop_front();
        }

        self.compute_trend(tick.timestamp, config.filters.trend_window_secs);

        let prev = self.tick_buf.back().copied();
        if prev.map(|p| (p - price).abs() > 0.0001).unwrap_or(true) {
            self.tick_buf.push_back(price);
            self.rsi.update(price);
            self.ema_short.update(price);
            self.ema_long.update(price);
            self.rolling_std.update(price);

            if self.tick_buf.len() > config.strategy.min_ticks {
                self.tick_buf.pop_front();
            }
        }

        // Opening range tracking (e.g. 09:15 - 09:20)
        let hour = tick.timestamp.hour();
        let min = tick.timestamp.minute();
        if hour == config.filters.session_start_hour
            && min >= config.filters.session_start_min
            && min < config.filters.session_start_min + (config.strategy.orb_window_mins as u32)
        {
            if self.opening_high == 0.0 || price > self.opening_high {
                self.opening_high = price;
            }
            if self.opening_low == 0.0 || price < self.opening_low {
                self.opening_low = price;
            }
        }

        let ctx = StrategyContext {
            current_price: self.current_price,
            volume: tick.volume,
            avg_volume: self.avg_volume(),
            last_trade_qty: self.last_trade_qty,
            avg_ltq: self.avg_ltq(),
            total_buy_qty: self.total_buy_qty,
            total_sell_qty: self.total_sell_qty,
            best_bid: self.best_bid,
            best_ask: self.best_ask,
            vwap: self.vwap.vwap,
            rsi: self.rsi.rsi,
            ema_short: self.ema_short.value(),
            std_dev: self.rolling_std.current().1,
            imbalance: tick.order_book_imbalance(),
            trend_direction: self.trend_direction,
            opening_high: self.opening_high,
            opening_low: self.opening_low,
            tick_buf: &self.tick_buf,
            price_history: &self.price_history,
        };

        // Confirmation of pending signal
        if let Some((side, strategy)) = self.pending_signal {
            self.confirmation_count += 1;
            if self.confirmation_count >= config.strategy.confirmation_ticks {
                let spread_ok = if self.best_bid > 0.0 && self.best_ask > 0.0 {
                    tick.spread_pct() <= config.filters.max_spread_pct
                } else {
                    true
                };

                let session_ok = hour > config.filters.session_start_hour
                    || (hour == config.filters.session_start_hour && min >= config.filters.session_start_min);

                self.pending_signal = None;
                self.confirmation_count = 0;

                if spread_ok && session_ok {
                    let uas = calculate_ies(&ctx, config);
                    self.last_alert = Some(tick.timestamp);
                    return Some(Signal {
                        symbol: self.symbol.clone(),
                        token: self.token,
                        price: self.current_price,
                        side,
                        strategy,
                        timestamp: tick.timestamp,
                        alpha_score: uas,
                    });
                }
            } else if self.confirmation_count > 10 {
                self.pending_signal = None;
                self.confirmation_count = 0;
            }
        } else {
            // Check alert cooldown
            let in_cooldown = if let Some(last) = self.last_alert {
                tick.timestamp.signed_duration_since(last).num_seconds() < config.filters.alert_cooldown_secs
            } else {
                false
            };

            if !in_cooldown {
                let strategies: [&dyn Strategy; 5] = [
                    &OpeningSniper,
                    &MeanReversion,
                    &LiquiditySweep,
                    &DepthPressure,
                    &MomentumScalp,
                ];

                for strat in &strategies {
                    if let Some(sig) = strat.evaluate(&ctx, tick.timestamp, config) {
                        self.pending_signal = Some(sig);
                        self.confirmation_count = 0;
                        break;
                    }
                }
            }
        }

        None
    }
}

#[derive(Debug, Clone)]
pub struct ChartData {
    pub symbol: String,
    pub price_points: Vec<(f64, f64)>,
    pub candles: Vec<OhlcvBar>,
    pub vwap_val: f64,
    pub rsi_val: f64,
    pub ema_val: f64,
    pub min_price: f64,
    pub max_price: f64,
    pub current_price: f64,
    pub total_buy_qty: u64,
    pub total_sell_qty: u64,
    pub best_bid: f64,
    pub best_ask: f64,
}

impl StockTracker {
    pub fn chart_data(&self) -> ChartData {
        let mut min_price = f64::MAX;
        let mut max_price = f64::MIN;
        let price_points: Vec<(f64, f64)> = self
            .tick_buf
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                if p < min_price { min_price = p; }
                if p > max_price { max_price = p; }
                (i as f64, p)
            })
            .collect();

        let candles = self.bar_aggregator.all_bars();
        for c in &candles {
            if c.low < min_price { min_price = c.low; }
            if c.high > max_price { max_price = c.high; }
        }

        if min_price > max_price {
            min_price = (self.current_price * 0.995).max(0.0);
            max_price = self.current_price * 1.005;
        } else if (max_price - min_price).abs() < 0.01 {
            min_price -= 0.5;
            max_price += 0.5;
        }

        ChartData {
            symbol: self.symbol.clone(),
            price_points,
            candles,
            vwap_val: self.vwap.vwap,
            rsi_val: self.rsi.rsi,
            ema_val: self.ema_short.value(),
            min_price,
            max_price,
            current_price: self.current_price,
            total_buy_qty: self.total_buy_qty,
            total_sell_qty: self.total_sell_qty,
            best_bid: self.best_bid,
            best_ask: self.best_ask,
        }
    }
}

pub struct StrategyEngine {
    pub trackers: HashMap<String, StockTracker>,
    pub config: Config,
}

impl StrategyEngine {
    pub fn new(config: Config) -> Self {
        Self {
            trackers: HashMap::new(),
            config,
        }
    }

    pub fn on_tick(&mut self, tick: &Tick) -> Option<Signal> {
        let tracker = self
            .trackers
            .entry(tick.symbol.clone())
            .or_insert_with(|| StockTracker::new(tick.symbol.clone(), tick.token));
        tracker.on_tick(tick, &self.config)
    }

    pub fn get_chart_data(&self, symbol: &str) -> Option<ChartData> {
        self.trackers.get(symbol).map(|t| t.chart_data())
    }
}

