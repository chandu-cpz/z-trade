use chrono::{DateTime, Local, Timelike};
use std::collections::VecDeque;

use trade_core::config::Config;
use trade_core::types::{PositionSide, StrategyType};
use trade_indicators::calculate_slope;

pub mod engine;
pub use engine::{StockTracker, StrategyEngine};

pub struct StrategyContext<'a> {
    pub current_price: f64,
    pub volume: u64,
    pub avg_volume: f64,
    pub last_trade_qty: u64,
    pub avg_ltq: f64,
    pub total_buy_qty: u64,
    pub total_sell_qty: u64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub vwap: f64,
    pub rsi: f64,
    pub ema_short: f64,
    pub std_dev: f64,
    pub imbalance: f64,
    pub trend_direction: i8,
    pub opening_high: f64,
    pub opening_low: f64,
    pub tick_buf: &'a VecDeque<f64>,
    pub price_history: &'a VecDeque<(f64, DateTime<Local>)>,
}

pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn evaluate(
        &self,
        ctx: &StrategyContext<'_>,
        now: DateTime<Local>,
        config: &Config,
    ) -> Option<(PositionSide, StrategyType)>;
}

pub struct OpeningSniper;
impl Strategy for OpeningSniper {
    fn name(&self) -> &'static str { "OpeningSniper" }
    fn evaluate(
        &self,
        ctx: &StrategyContext<'_>,
        now: DateTime<Local>,
        config: &Config,
    ) -> Option<(PositionSide, StrategyType)> {
        let hour = now.hour();
        let min = now.minute();
        let orb_end_min = config.filters.session_start_min + (config.strategy.orb_window_mins as u32);

        if (hour == config.filters.session_start_hour && min >= orb_end_min) || hour > config.filters.session_start_hour {
            if ctx.opening_high > 0.0 && ctx.opening_low > 0.0 {
                if ctx.current_price > ctx.opening_high {
                    return Some((PositionSide::Long, StrategyType::OpeningSniper));
                }
                if ctx.current_price < ctx.opening_low {
                    return Some((PositionSide::Short, StrategyType::OpeningSniper));
                }
            }
        }
        None
    }
}

pub struct MeanReversion;
impl Strategy for MeanReversion {
    fn name(&self) -> &'static str { "MeanReversion" }
    fn evaluate(
        &self,
        ctx: &StrategyContext<'_>,
        _now: DateTime<Local>,
        config: &Config,
    ) -> Option<(PositionSide, StrategyType)> {
        if ctx.tick_buf.len() < 20 {
            return None;
        }

        let price = ctx.current_price;
        let rsi = ctx.rsi;
        let ema = ctx.ema_short;
        let std = ctx.std_dev;
        let slope = calculate_slope(ctx.tick_buf);
        let imbalance = ctx.imbalance;
        let vwap = ctx.vwap;

        let upper_band = ema + (2.5 * std);
        let lower_band = ema - (2.5 * std);
        let upper_vwap_band = if std > 0.0 { vwap + (config.strategy.vwap_stdev_mult * std) } else { f64::MAX };
        let lower_vwap_band = if std > 0.0 { vwap - (config.strategy.vwap_stdev_mult * std) } else { f64::MIN };

        // Exhaustion Short
        if rsi > 75.0 && (price > upper_band || price > upper_vwap_band) {
            if slope < 0.01 && imbalance < -0.1 {
                return Some((PositionSide::Short, StrategyType::MeanReversion));
            }
        }

        // Exhaustion Long
        if rsi < 25.0 && (price < lower_band || price < lower_vwap_band) {
            if slope > -0.01 && imbalance > 0.1 {
                return Some((PositionSide::Long, StrategyType::MeanReversion));
            }
        }

        None
    }
}

pub struct LiquiditySweep;
impl Strategy for LiquiditySweep {
    fn name(&self) -> &'static str { "LiquiditySweep" }
    fn evaluate(
        &self,
        ctx: &StrategyContext<'_>,
        _now: DateTime<Local>,
        config: &Config,
    ) -> Option<(PositionSide, StrategyType)> {
        if ctx.opening_high == 0.0 || ctx.opening_low == 0.0 {
            return None;
        }

        let price = ctx.current_price;
        let high = ctx.opening_high;
        let low = ctx.opening_low;
        let pierce_pct = config.strategy.liq_sweep_tick_pierce;
        let imbalance = ctx.imbalance;

        // High Sweep trap -> Short
        if price > high && price < high * (1.0 + (pierce_pct / 100.0)) && ctx.rsi > 70.0 && imbalance < -0.2 {
            return Some((PositionSide::Short, StrategyType::LiquiditySweep));
        }

        // Low Sweep trap -> Long
        if price < low && price > low * (1.0 - (pierce_pct / 100.0)) && ctx.rsi < 30.0 && imbalance > 0.2 {
            return Some((PositionSide::Long, StrategyType::LiquiditySweep));
        }

        None
    }
}

pub struct DepthPressure;
impl Strategy for DepthPressure {
    fn name(&self) -> &'static str { "DepthPressure" }
    fn evaluate(
        &self,
        ctx: &StrategyContext<'_>,
        _now: DateTime<Local>,
        config: &Config,
    ) -> Option<(PositionSide, StrategyType)> {
        if ctx.total_buy_qty == 0 || ctx.total_sell_qty == 0 {
            return None;
        }

        let ratio = ctx.total_buy_qty as f64 / ctx.total_sell_qty as f64;
        let price_flat = calculate_slope(ctx.tick_buf).abs() < 0.01;

        if ratio >= config.strategy.ofi_imbalance_ratio && price_flat {
            return Some((PositionSide::Long, StrategyType::DepthPressure));
        }
        if ratio <= (1.0 / config.strategy.ofi_imbalance_ratio) && price_flat {
            return Some((PositionSide::Short, StrategyType::DepthPressure));
        }

        None
    }
}

pub struct MomentumScalp;
impl Strategy for MomentumScalp {
    fn name(&self) -> &'static str { "MomentumScalp" }
    fn evaluate(
        &self,
        ctx: &StrategyContext<'_>,
        _now: DateTime<Local>,
        config: &Config,
    ) -> Option<(PositionSide, StrategyType)> {
        if ctx.tick_buf.len() < config.strategy.min_ticks {
            return None;
        }

        let slope = calculate_slope(ctx.tick_buf);
        let price = ctx.current_price;
        let vwap = if ctx.vwap > 0.0 { ctx.vwap } else { price };
        let imbalance = ctx.imbalance;
        let rsi = ctx.rsi;

        let trend_up = ctx.trend_direction == 1;
        let trend_dn = ctx.trend_direction == -1;

        let check_long = price >= vwap && imbalance > 0.1 && slope > -0.05 && trend_up && rsi < 75.0;
        let check_short = price <= vwap && imbalance < -0.1 && slope < 0.05 && trend_dn && rsi > 25.0;

        // V-Shape Rip / Drop
        let buf_len = ctx.tick_buf.len();
        let need = config.strategy.consec_ticks + 1;
        if buf_len >= need {
            let window: Vec<f64> = ctx.tick_buf.iter().rev().take(need).cloned().collect();
            let mut up_moves = 0usize;
            let mut dn_moves = 0usize;
            for i in 0..(window.len() - 1) {
                if window[i] > window[i + 1] {
                    up_moves += 1;
                } else if window[i] < window[i + 1] {
                    dn_moves += 1;
                }
            }

            let is_vol_burst = ctx.avg_volume >= 1.0
                && (ctx.volume as f64) >= (ctx.avg_volume * config.strategy.vol_burst_mult);

            if up_moves >= config.strategy.consec_ticks && dn_moves == 0 && is_vol_burst && check_long {
                return Some((PositionSide::Long, StrategyType::VShapeRip));
            }
            if dn_moves >= config.strategy.consec_ticks && up_moves == 0 && is_vol_burst && check_short {
                return Some((PositionSide::Short, StrategyType::VShapeDrop));
            }
        }

        // Acceleration Burst
        if buf_len >= 6 {
            let oldest = ctx.tick_buf[buf_len - 6];
            let newest = ctx.tick_buf[buf_len - 1];
            if oldest > 0.0 {
                let accel_pct = (newest - oldest) / oldest * 100.0;
                let ltq_spike = ctx.avg_ltq >= 1.0
                    && (ctx.last_trade_qty as f64) >= (ctx.avg_ltq * config.strategy.ltq_burst_mult);

                if accel_pct.abs() >= config.strategy.accel_thresh && ltq_spike {
                    if accel_pct > 0.0 && check_long {
                        return Some((PositionSide::Long, StrategyType::MomentumScalp));
                    } else if accel_pct < 0.0 && check_short {
                        return Some((PositionSide::Short, StrategyType::MomentumScalp));
                    }
                }
            }
        }

        None
    }
}

/// Calculate Impulse Energy Score (IES / UAS) [0.0 - 100.0].
pub fn calculate_ies(ctx: &StrategyContext<'_>, config: &Config) -> f64 {
    if ctx.price_history.len() < config.impulse_scoring.ies_window_ticks {
        return 0.0;
    }

    let oldest_idx = ctx.price_history.len() - config.impulse_scoring.ies_window_ticks;
    let oldest_price = ctx.price_history[oldest_idx].0;
    let price_diff = if oldest_price > 0.0 {
        (ctx.current_price - oldest_price).abs() / oldest_price
    } else {
        0.0
    };
    let velocity_score = (price_diff * 10000.0).min(50.0);

    let volume_score = if ctx.last_trade_qty > 500 {
        20.0
    } else if ctx.last_trade_qty > 100 {
        10.0
    } else {
        0.0
    };

    let ofi_score = {
        let ratio = if ctx.total_sell_qty > 0 {
            ctx.total_buy_qty as f64 / ctx.total_sell_qty as f64
        } else {
            10.0
        };
        if ratio > 4.0 || ratio < 0.25 {
            30.0
        } else if ratio > 2.0 || ratio < 0.5 {
            15.0
        } else {
            0.0
        }
    };

    let spread = (ctx.best_ask - ctx.best_bid).abs();
    let spread_pct = if ctx.current_price > 0.0 {
        (spread / ctx.current_price) * 100.0
    } else {
        1.0
    };
    let spread_mult = if spread_pct < 0.03 {
        1.2
    } else if spread_pct < 0.08 {
        1.0
    } else {
        0.7
    };

    ((velocity_score + volume_score + ofi_score) * spread_mult).clamp(0.0, 100.0)
}
