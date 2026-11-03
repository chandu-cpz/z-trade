use chrono::{DateTime, Local, Timelike};
use std::collections::VecDeque;

use crate::config::Config;
use crate::indicators::calculate_slope;
use crate::models::{PositionSide, PricePoint, Strategy};

/// Context parameters passed into strategy detectors.
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
    pub price_history: &'a VecDeque<PricePoint>,
}

/// Detects opening range breakout after the initial opening window (e.g., 09:20+).
pub fn detect_opening_sniper(
    ctx: &StrategyContext<'_>,
    now: DateTime<Local>,
    config: &Config,
) -> Option<(PositionSide, Strategy)> {
    let hour = now.hour();
    let min = now.minute();
    let orb_end_min = config.session_start_min + (config.orb_window_mins as u32);

    if (hour == config.session_start_hour && min >= orb_end_min) || hour > config.session_start_hour {
        if ctx.opening_high > 0.0 && ctx.opening_low > 0.0 {
            if ctx.current_price > ctx.opening_high {
                return Some((PositionSide::Long, Strategy::OpeningSniper));
            }
            if ctx.current_price < ctx.opening_low {
                return Some((PositionSide::Short, Strategy::OpeningSniper));
            }
        }
    }
    None
}

/// Detects mean reversion when price is stretched far beyond Bollinger or VWAP bands
/// with RSI exhaustion, slope flattening, and order book confluence.
pub fn detect_mean_reversion(
    ctx: &StrategyContext<'_>,
    config: &Config,
) -> Option<(PositionSide, Strategy)> {
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
    let upper_vwap_band = if std > 0.0 { vwap + (config.vwap_stdev_mult * std) } else { f64::MAX };
    let lower_vwap_band = if std > 0.0 { vwap - (config.vwap_stdev_mult * std) } else { f64::MIN };

    // Exhaustion Short:
    // 1. Price above upper band or VWAP band
    // 2. High RSI (overbought > 75.0)
    // 3. Slope flattening/dropping
    // 4. Sellers taking over the order book (imbalance < -0.1)
    if rsi > 75.0 && (price > upper_band || price > upper_vwap_band) {
        if slope < 0.01 && imbalance < -0.1 {
            return Some((PositionSide::Short, Strategy::MeanReversion));
        }
    }

    // Exhaustion Long:
    // 1. Price below lower band or VWAP band
    // 2. Low RSI (oversold < 25.0)
    // 3. Slope flattening/rising
    // 4. Buyers stepping in (imbalance > 0.1)
    if rsi < 25.0 && (price < lower_band || price < lower_vwap_band) {
        if slope > -0.01 && imbalance > 0.1 {
            return Some((PositionSide::Long, Strategy::MeanReversion));
        }
    }

    None
}

/// Detects fake breakout / liquidity sweep where price pierces the opening range
/// but is immediately absorbed/rejected in the order book.
pub fn detect_liquidity_sweep(
    ctx: &StrategyContext<'_>,
    config: &Config,
) -> Option<(PositionSide, Strategy)> {
    if ctx.opening_high == 0.0 || ctx.opening_low == 0.0 {
        return None;
    }

    let price = ctx.current_price;
    let high = ctx.opening_high;
    let low = ctx.opening_low;
    let pierce_pct = config.liq_sweep_tick_pierce;
    let imbalance = ctx.imbalance;

    // High sweep: pierce high, rsi high, sellers trap top (imbalance < -0.2)
    if price > high && price < high * (1.0 + (pierce_pct / 100.0)) && ctx.rsi > 70.0 && imbalance < -0.2 {
        return Some((PositionSide::Short, Strategy::LiquiditySweep));
    }

    // Low sweep: pierce low, rsi low, buyers step in (imbalance > 0.2)
    if price < low && price > low * (1.0 - (pierce_pct / 100.0)) && ctx.rsi < 30.0 && imbalance > 0.2 {
        return Some((PositionSide::Long, Strategy::LiquiditySweep));
    }

    None
}

/// Detects order flow pressure (OFI ratio extreme with flat price).
pub fn detect_depth_pressure(
    ctx: &StrategyContext<'_>,
    config: &Config,
) -> Option<(PositionSide, Strategy)> {
    if ctx.total_buy_qty == 0 || ctx.total_sell_qty == 0 {
        return None;
    }

    let ratio = ctx.total_buy_qty as f64 / ctx.total_sell_qty as f64;
    let price_flat = calculate_slope(ctx.tick_buf).abs() < 0.01;

    if ratio >= config.ofi_imbalance_ratio && price_flat {
        return Some((PositionSide::Long, Strategy::DepthPressure));
    }
    if ratio <= (1.0 / config.ofi_imbalance_ratio) && price_flat {
        return Some((PositionSide::Short, Strategy::DepthPressure));
    }

    None
}

/// Detects momentum scalp setups: VShapeRip/Drop or acceleration burst.
pub fn detect_scalp_momentum(
    ctx: &StrategyContext<'_>,
    config: &Config,
) -> Option<(PositionSide, Strategy)> {
    if ctx.tick_buf.len() < config.min_ticks {
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

    // Strategy 1: V-Shape Rip / Drop
    let buf_len = ctx.tick_buf.len();
    let need = config.consec_ticks + 1;
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
            && (ctx.volume as f64) >= (ctx.avg_volume * config.vol_burst_mult);

        if up_moves >= config.consec_ticks && dn_moves == 0 && is_vol_burst && check_long {
            return Some((PositionSide::Long, Strategy::VShapeRip));
        }
        if dn_moves >= config.consec_ticks && up_moves == 0 && is_vol_burst && check_short {
            return Some((PositionSide::Short, Strategy::VShapeDrop));
        }
    }

    // Strategy 2: Acceleration Scalp
    if buf_len >= 6 {
        let oldest = ctx.tick_buf[buf_len - 6];
        let newest = ctx.tick_buf[buf_len - 1];
        if oldest > 0.0 {
            let accel_pct = (newest - oldest) / oldest * 100.0;
            let ltq_spike = ctx.avg_ltq >= 1.0
                && (ctx.last_trade_qty as f64) >= (ctx.avg_ltq * config.ltq_burst_mult);

            if accel_pct.abs() >= config.accel_thresh && ltq_spike {
                if accel_pct > 0.0 && check_long {
                    return Some((PositionSide::Long, Strategy::MomentumScalp));
                } else if accel_pct < 0.0 && check_short {
                    return Some((PositionSide::Short, Strategy::MomentumScalp));
                }
            }
        }
    }

    None
}

/// Computes Impulse Energy Score (IES / UAS) [0.0 - 100.0].
pub fn calculate_ies(ctx: &StrategyContext<'_>, config: &Config) -> f64 {
    if ctx.price_history.len() < config.ies_window_ticks {
        return 0.0;
    }

    // 1. Price Velocity
    let oldest_idx = ctx.price_history.len() - config.ies_window_ticks;
    let oldest_price = ctx.price_history[oldest_idx].price;
    let price_diff = if oldest_price > 0.0 {
        (ctx.current_price - oldest_price).abs() / oldest_price
    } else {
        0.0
    };
    let velocity_score = (price_diff * 10000.0).min(50.0);

    // 2. Volume & Trade Size Surge
    let volume_score = if ctx.last_trade_qty > 500 {
        20.0
    } else if ctx.last_trade_qty > 100 {
        10.0
    } else {
        0.0
    };

    // 3. Order Book Pressure (OFI)
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

    // 4. Spread Multiplier
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
