use crate::models::*;
use chrono::{DateTime, Local, Timelike};
use std::collections::{VecDeque, HashMap};
use std::sync::mpsc::{Sender, Receiver};

// ═══════════════════════════════════════════════════════════════════════════
// MESSAGES
// ═══════════════════════════════════════════════════════════════════════════

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

#[derive(Debug)]
pub struct SignalMsg {
    pub symbol: String,
    pub price: f64,
    pub side: PositionSide,
    pub strategy: Strategy,
    pub timestamp: DateTime<Local>,
    pub uas: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// STOCK TRACKER (Parallel Version)
// ═══════════════════════════════════════════════════════════════════════════

pub struct StockTracker {
    pub _symbol: String,
    pub price_history: VecDeque<PricePoint>,
    pub volume_history: VecDeque<u64>,
    pub current_price: f64,
    pub last_alert: Option<DateTime<Local>>,
    // Signal queued for confirmation (mode is always ScalpMom now)
    pub pending_signal: Option<(PositionSide, Strategy)>,
    pub confirmation_count: usize,
    // Order flow tracking
    pub total_buy_qty: u64,   
    pub total_sell_qty: u64,  
    pub prev_buy_qty: u64,    
    pub prev_sell_qty: u64,   
    pub best_bid: f64,        
    pub best_ask: f64,        
    pub last_trade_qty: u64,  
    pub ltq_history: VecDeque<u64>, 
    // Trend context
    pub trend_direction: i8,  
    pub _last_loss_time: Option<DateTime<Local>>,
    // Short rolling tick buffer
    pub tick_buf: VecDeque<f64>,
    // Smart Scalp fields
    pub cumulative_pv: f64,
    pub cumulative_vol: u64,
    pub vwap: f64,
    pub imbalance: f64,
    // Technical Indicators
    pub rsi: f64,
    pub avg_gain: f64,
    pub avg_loss: f64,
    pub ema_short: f64, // 13-period
    pub ema_long: f64,  // 50-period
    pub std_dev: f64,
    // ORB Tracking
    pub opening_high: f64,
    pub opening_low: f64,
    // Signal Confluence Tracking
    pub last_ofi_time: Option<DateTime<Local>>,
    pub last_mom_time: Option<DateTime<Local>>,
}

impl StockTracker {
    pub fn new(symbol: String) -> Self {
        Self {
            _symbol: symbol,
            price_history: VecDeque::with_capacity(200),
            volume_history: VecDeque::with_capacity(200),
            current_price: 0.0,
            last_alert: None,
            pending_signal: None,
            confirmation_count: 0,
            total_buy_qty: 0,
            total_sell_qty: 0,
            prev_buy_qty: 0,
            prev_sell_qty: 0,
            best_bid: 0.0,
            best_ask: 0.0,
            last_trade_qty: 0,
            ltq_history: VecDeque::with_capacity(50),
            trend_direction: 0,
            _last_loss_time: None,
            tick_buf: VecDeque::with_capacity(50),
            cumulative_pv: 0.0,
            cumulative_vol: 0,
            vwap: 0.0,
            imbalance: 0.0,
            rsi: 50.0,
            avg_gain: 0.0,
            avg_loss: 0.0,
            ema_short: 0.0,
            ema_long: 0.0,
            std_dev: 0.0,
            opening_high: 0.0,
            opening_low: 0.0,
            last_ofi_time: None,
            last_mom_time: None,
        }
    }

    pub fn calculate_slope(&self) -> f64 {
        let n = self.tick_buf.len();
        if n < 5 { return 0.0; }
        let n_f = n as f64;
        let sum_x: f64 = (0..n).map(|i| i as f64).sum();
        let sum_y: f64 = self.tick_buf.iter().sum();
        let sum_xy: f64 = self.tick_buf.iter().enumerate().map(|(i, &p)| (i as f64) * p).sum();
        let sum_xx: f64 = (0..n).map(|i| (i as f64).powi(2)).sum();
        
        let numerator = n_f * sum_xy - sum_x * sum_y;
        let denominator = n_f * sum_xx - sum_x.powi(2);
        
        if denominator == 0.0 { 0.0 } else { numerator / denominator }
    }

    pub fn update(&mut self, price: f64, volume: u64, tbq: u64, tsq: u64, bp: f64, sp: f64, ltq: u64, now: DateTime<Local>) {
        self.current_price = price;

        // VWAP Update
        if volume > 0 {
            self.cumulative_pv += price * (volume as f64);
            self.cumulative_vol += volume;
            if self.cumulative_vol > 0 {
                self.vwap = self.cumulative_pv / (self.cumulative_vol as f64);
            }
        }

        // Snapshot previous order book
        if self.total_buy_qty > 0 { self.prev_buy_qty = self.total_buy_qty; }
        if self.total_sell_qty > 0 { self.prev_sell_qty = self.total_sell_qty; }

        // Order flow
        if tbq > 0 { self.total_buy_qty = tbq; }
        if tsq > 0 { self.total_sell_qty = tsq; }
        
        // Imbalance Update
        let total_qty = (self.total_buy_qty + self.total_sell_qty) as f64;
        if total_qty > 0.0 {
            self.imbalance = (self.total_buy_qty as f64 - self.total_sell_qty as f64) / total_qty;
        }

        if bp > 0.0 { self.best_bid = bp; }
        if sp > 0.0 { self.best_ask = sp; }
        if ltq > 0 {
            self.last_trade_qty = ltq;
            self.ltq_history.push_back(ltq);
            if self.ltq_history.len() > 60 {
                self.ltq_history.pop_front();
            }
        }

        self.price_history.push_back(PricePoint { price, timestamp: now });
        self.volume_history.push_back(volume);

        // Limit history - cheap O(1) checks
        if self.price_history.len() > 3600 {
             while let Some(oldest) = self.price_history.front() {
                if now.signed_duration_since(oldest.timestamp).num_seconds() > TREND_WINDOW_SECS + 60 {
                    self.price_history.pop_front();
                } else {
                    break;
                }
             }
        }

        if self.volume_history.len() > 120 {
            self.volume_history.pop_front();
        }

        // Update 5-min trend direction
        self.compute_trend(now);

        // Update short tick buffer
        let prev_price = self.tick_buf.back().cloned();
        if prev_price.map(|last| (last - price).abs() > 0.0001).unwrap_or(true) {
            self.tick_buf.push_back(price);
            
            // ── Technical Indicators Calculation ───────────────────────────
            if let Some(prev) = prev_price {
                let diff = price - prev;
                let gain = if diff > 0.0 { diff } else { 0.0 };
                let loss = if diff < 0.0 { -diff } else { 0.0 };

                // Alpha for RSI (1/14) and EMA
                let rsi_alpha = 1.0 / 14.0;
                self.avg_gain = (gain * rsi_alpha) + (self.avg_gain * (1.0 - rsi_alpha));
                self.avg_loss = (loss * rsi_alpha) + (self.avg_loss * (1.0 - rsi_alpha));
                
                if self.avg_loss > 0.0 {
                    let rs = self.avg_gain / self.avg_loss;
                    self.rsi = 100.0 - (100.0 / (1.0 + rs));
                } else if self.avg_gain > 0.0 {
                    self.rsi = 100.0;
                } else {
                    self.rsi = 50.0;
                }

                // EMAs
                if self.ema_short == 0.0 { self.ema_short = price; }
                if self.ema_long == 0.0 { self.ema_long = price; }
                
                let alpha_short = 2.0 / (13.0 + 1.0);
                let alpha_long = 2.0 / (50.0 + 1.0);
                self.ema_short = (price * alpha_short) + (self.ema_short * (1.0 - alpha_short));
                self.ema_long = (price * alpha_long) + (self.ema_long * (1.0 - alpha_long));
                
                // Std Dev (window of tick_buf)
                let n = self.tick_buf.len() as f64;
                let mean = self.tick_buf.iter().sum::<f64>() / n;
                let variance = self.tick_buf.iter().map(|&p| (p - mean).powi(2)).sum::<f64>() / n;
                self.std_dev = variance.sqrt();
            }

            if self.tick_buf.len() > SCALP_MIN_TICKS {
                self.tick_buf.pop_front();
            }
        }

        // ── ORB Range Definition (9:15 - 9:20) ──────────────────────────
        // Only if we are in the first 5 mins of the market (9:15-9:20)
        let hour = now.hour();
        let min = now.minute();
        if hour == 9 && min >= 15 && min < 15 + (ORB_WINDOW_MINS as u32) {
             if self.opening_high == 0.0 || price > self.opening_high { self.opening_high = price; }
             if self.opening_low == 0.0 || price < self.opening_low { self.opening_low = price; }
        }
    }

    pub fn compute_trend(&mut self, now: DateTime<Local>) {
        if self.price_history.is_empty() { return; }
        
        // Optimally find trend origin
        let trend_start = now - chrono::Duration::seconds(TREND_WINDOW_SECS);
        let oldest_in_trend = self.price_history.iter().find(|pp| pp.timestamp >= trend_start);
            
        if let Some(old) = oldest_in_trend {
            if old.price > 0.0 {
                let change = (self.current_price - old.price) / old.price * 100.0;
                self.trend_direction = if change > 0.10 { 1 } else if change < -0.10 { -1 } else { 0 };
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
        if self.ltq_history.is_empty() { return 0.0; }
        self.ltq_history.iter().sum::<u64>() as f64 / self.ltq_history.len() as f64
    }

    pub fn detect_mean_reversion(&self) -> Option<(PositionSide, Strategy)> {
        if self.tick_buf.len() < 20 { return None; }

        let price = self.current_price;
        let rsi = self.rsi;
        let ema = self.ema_short;
        let std = self.std_dev;
        
        // Thresholds for "extreme" expansion
        let upper_band = ema + (2.5 * std);
        let lower_band = ema - (2.5 * std);

        if rsi > 80.0 && price > upper_band {
            // Check for exhaustion: last 2 ticks slowing down
            return Some((PositionSide::Short, Strategy::MeanReversion));
        }
        
        if rsi < 20.0 && price < lower_band {
            return Some((PositionSide::Long, Strategy::MeanReversion));
        }

        None
    }

    pub fn detect_scalp_momentum(&mut self, volume: u64) -> Option<(PositionSide, Strategy)> {
        if self.tick_buf.len() < SCALP_MIN_TICKS {
            return None;
        }

        let slope = self.calculate_slope();
        let price = self.current_price;
        let vwap = if self.vwap > 0.0 { self.vwap } else { price };
        let imbalance = self.imbalance; 
        let rsi = self.rsi;

        let trend_up = self.trend_direction == 1;
        let trend_dn = self.trend_direction == -1;

        // Momentum quality filters: don't chase if RSI is already extreme
        let check_long = price >= vwap && imbalance > -0.3 && slope > -0.05 && trend_up && rsi < 75.0;
        let check_short = price <= vwap && imbalance < 0.3 && slope < 0.05 && trend_dn && rsi > 25.0;

        let avg_vol = self.avg_volume();
        let avg_ltq = self.avg_ltq();

        // ── Strategy A & B: VShapeRip / VShapeDrop ───────────────────────────
        let buf_len = self.tick_buf.len();
        let need = SCALP_CONSEC_TICKS + 1;
        if buf_len >= need {
            let window: Vec<f64> = self.tick_buf.iter().rev().take(need).cloned().collect();
            let mut up_moves = 0usize;
            let mut dn_moves = 0usize;
            for i in 0..(window.len() - 1) {
                if window[i] > window[i + 1] { up_moves += 1; }
                else if window[i] < window[i + 1] { dn_moves += 1; }
            }

            let moves_needed = SCALP_CONSEC_TICKS;
            let is_vol_burst = avg_vol >= 1.0 && volume as f64 >= avg_vol * SCALP_VOL_BURST_MULT;

            if up_moves >= moves_needed && dn_moves == 0 && is_vol_burst && check_long {
                return Some((PositionSide::Long, Strategy::VShapeRip));
            }
            if dn_moves >= moves_needed && up_moves == 0 && is_vol_burst && check_short {
                return Some((PositionSide::Short, Strategy::VShapeDrop));
            }
        }

        // ── Strategy C: MomentumScalp ─────────────────────────────────────────
        if buf_len >= 6 {
            let oldest = self.tick_buf[buf_len - 6];
            let newest = self.tick_buf[buf_len - 1];
            if oldest > 0.0 {
                let accel_pct = (newest - oldest) / oldest * 100.0;
                let ltq_spike = avg_ltq >= 1.0
                    && self.last_trade_qty as f64 >= avg_ltq * SCALP_LTQ_BURST_MULT;
                
                if accel_pct.abs() >= SCALP_ACCEL_THRESH && ltq_spike {
                    self.last_mom_time = Some(Local::now());
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

    pub fn detect_opening_sniper(&self, now: DateTime<Local>) -> Option<(PositionSide, Strategy)> {
        // Only valid AFTER 5 mins (e.g. 09:20 onwards)
        let hour = now.hour();
        let min = now.minute();
        if (hour == 9 && min >= 15 + (ORB_WINDOW_MINS as u32)) || hour > 9 {
             if self.opening_high > 0.0 && self.opening_low > 0.0 {
                 if self.current_price > self.opening_high {
                     return Some((PositionSide::Long, Strategy::OpeningSniper));
                 }
                 if self.current_price < self.opening_low {
                     return Some((PositionSide::Short, Strategy::OpeningSniper));
                 }
             }
        }
        None
    }

    pub fn detect_depth_pressure(&mut self) -> Option<(PositionSide, Strategy)> {
        // OFI: TBQ vs TSQ Imbalance
        if self.total_buy_qty == 0 || self.total_sell_qty == 0 { return None; }
        
        let ratio = self.total_buy_qty as f64 / self.total_sell_qty as f64;
        let price_flat = self.calculate_slope().abs() < 0.01;

        if ratio >= OFI_IMBALANCE_RATIO && price_flat {
            self.last_ofi_time = Some(Local::now());
            return Some((PositionSide::Long, Strategy::DepthPressure));
        }
        if ratio <= (1.0 / OFI_IMBALANCE_RATIO) && price_flat {
            self.last_ofi_time = Some(Local::now());
            return Some((PositionSide::Short, Strategy::DepthPressure));
        }
        None
    }

    pub fn detect_vwap_magnet(&self) -> Option<(PositionSide, Strategy)> {
        if self.vwap == 0.0 || self.std_dev == 0.0 { return None; }
        
        let price = self.current_price;
        let vwap = self.vwap;
        let dev = self.std_dev;
        let rsi = self.rsi;

        // Reversion to VWAP from 3.0 StDev away
        if price > vwap + (VWAP_STDEV_MULT * dev) && rsi > 75.0 {
            return Some((PositionSide::Short, Strategy::VWAPMagnet));
        }
        if price < vwap - (VWAP_STDEV_MULT * dev) && rsi < 25.0 {
            return Some((PositionSide::Long, Strategy::VWAPMagnet));
        }
        None
    }

    pub fn detect_liquidity_sweep(&self) -> Option<(PositionSide, Strategy)> {
        // Detect "fake" breakout: price pierces range high but returns quickly
        if self.opening_high == 0.0 || self.opening_low == 0.0 { return None; }
        
        let price = self.current_price;
        let high = self.opening_high;
        let low = self.opening_low;
        let pierce_pct = LIQ_SWEEP_TICK_PIERCE;
        
        // High Sweep: pierce high, rsi turning down
        if price > high && price < high * (1.0 + (pierce_pct/100.0)) && self.rsi > 70.0 {
             // We bet on reversal
             return Some((PositionSide::Short, Strategy::LiquiditySweep));
        }
        // Low Sweep: pierce low, rsi turning up
        if price < low && price > low * (1.0 - (pierce_pct/100.0)) && self.rsi < 30.0 {
             return Some((PositionSide::Long, Strategy::LiquiditySweep));
        }
        None
    }

    pub fn calculate_ies(&self) -> f64 {
        if self.price_history.len() < IES_WINDOW_TICKS { return 0.0; }
        
        // 1. Price Velocity (Steepness of the burst)
        let oldest_idx = self.price_history.len() - IES_WINDOW_TICKS;
        let oldest_price = self.price_history[oldest_idx].price;
        let price_diff = (self.current_price - oldest_price).abs() / oldest_price;
        let velocity_score = (price_diff * 10000.0).min(50.0); // 0.1% move = 10 pts

        // 2. LTQ & Volume Surge
        let volume_score = if self.last_trade_qty > 500 { 20.0 } else if self.last_trade_qty > 100 { 10.0 } else { 0.0 };

        // 3. Book Pressure (OFI)
        let ofi_score = {
            let ratio = if self.total_sell_qty > 0 { self.total_buy_qty as f64 / self.total_sell_qty as f64 } else { 10.0 };
            if ratio > 4.0 || ratio < 0.25 { 30.0 } else if ratio > 2.0 || ratio < 0.5 { 15.0 } else { 0.0 }
        };

        // 4. Spread Multiplier (Tight spreads = better energy transfer)
        let spread = (self.best_ask - self.best_bid).abs();
        let spread_pct = if self.current_price > 0.0 { (spread / self.current_price) * 100.0 } else { 1.0 };
        let spread_mult = if spread_pct < 0.03 { 1.2 } else if spread_pct < 0.08 { 1.0 } else { 0.7 };

        ((velocity_score + volume_score + ofi_score) * spread_mult).clamp(0.0, 100.0)
    }

    pub fn calculate_uas(&self, _strategy: Strategy) -> f64 {
        self.calculate_ies()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WORKER THREAD LOGIC
// ═══════════════════════════════════════════════════════════════════════════

pub struct Worker {
    pub _id: usize,
    pub trackers: HashMap<String, StockTracker>,
    pub signal_tx: Sender<SignalMsg>,
}

impl Worker {
    pub fn new(id: usize, signal_tx: Sender<SignalMsg>) -> Self {
        Self {
            _id: id,
            trackers: HashMap::new(),
            signal_tx,
        }
    }

    pub fn run(mut self, rx: Receiver<WorkerMsg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                WorkerMsg::Terminate => break,
                WorkerMsg::Tick { token, price, volume, total_buy_qty, total_sell_qty, best_bid, best_ask, last_trade_qty, timestamp } => {
                    let tracker = self.trackers.entry(token.clone()).or_insert_with(|| StockTracker::new(token.clone()));
                    
                    tracker.update(price, volume, total_buy_qty, total_sell_qty, best_bid, best_ask, last_trade_qty, timestamp);

                    // Check for confirmation
                    if let Some((pending_side, _pending_strategy)) = tracker.pending_signal {
                        tracker.confirmation_count += 1;
                        if tracker.confirmation_count >= SCALP_CONFIRMATION {
                            
                            // ── FILTERS (Pre-Dispatch) ───────────────────────────
                            // Gate 1: Spread Filter
                            let spread_ok = if tracker.best_bid > 0.0 && tracker.best_ask > 0.0 {
                                ((tracker.best_ask - tracker.best_bid) / tracker.best_bid) * 100.0 <= MAX_SPREAD_PCT
                            } else {
                                true 
                            };

                            // Gate 2: Session Filter (09:30)
                            let session_ok = timestamp.hour() > SESSION_START_HOUR
                                || (timestamp.hour() == SESSION_START_HOUR && timestamp.minute() >= SESSION_START_MIN);

                            if spread_ok && session_ok {
                                // Apply Strategy Inversion if enabled
                                let final_side = if INVERT_STRATEGY {
                                    match pending_side {
                                        PositionSide::Long => PositionSide::Short,
                                        PositionSide::Short => PositionSide::Long,
                                    }
                                } else {
                                    pending_side
                                };

                                // FIRE SIGNAL
                                if let Some((_side, strategy)) = tracker.pending_signal {
                                    let uas = tracker.calculate_uas(strategy);
                                    let _ = self.signal_tx.send(SignalMsg {
                                        symbol: token.clone(),
                                        price: tracker.current_price,
                                        side: final_side, // Use final_side after inversion logic
                                        strategy,
                                        timestamp,
                                        uas,
                                    });
                                    tracker.pending_signal = None; // Reset
                                }
                                tracker.last_alert = Some(timestamp);
                            } else {
                                // Dropped silently or logged?
                                // Could send "BlockedSignal" if we want to visualize it
                            }

                            tracker.pending_signal = None;
                            tracker.confirmation_count = 0;
                        } else {
                            // Signal Aging: If pending but not confirmed in 10 ticks, clear it
                            if tracker.confirmation_count > 10 {
                                tracker.pending_signal = None;
                                tracker.confirmation_count = 0;
                            }
                        }
                    } else {
                        // Check for new signal
                         let cooldown = SCALP_ALERT_COOLDOWN;
                         let in_cooldown = if let Some(last) = tracker.last_alert {
                             timestamp.signed_duration_since(last).num_seconds() < cooldown
                         } else { false };
                         
                         if !in_cooldown {
                             // --- Multi-Strategy Alpha Pack Prioritization ---
                             
                             // 1. Opening Sniper (Rank 1: Opening Hour only)
                             if let Some(sig) = tracker.detect_opening_sniper(timestamp) {
                                 tracker.pending_signal = Some(sig);
                             }
                             // 2. High-Edge Reversions (Rank 2)
                             else if let Some(sig) = tracker.detect_vwap_magnet() {
                                 tracker.pending_signal = Some(sig);
                             }
                             else if let Some(sig) = tracker.detect_mean_reversion() {
                                 tracker.pending_signal = Some(sig);
                             }
                             // 3. Reversal Traps (Rank 3)
                             else if let Some(sig) = tracker.detect_liquidity_sweep() {
                                 tracker.pending_signal = Some(sig);
                             }
                             // 4. Order Flow (Rank 4)
                             else if let Some(sig) = tracker.detect_depth_pressure() {
                                 tracker.pending_signal = Some(sig);
                             }
                             // 5. Momentum Follow (Rank 5)
                             else if let Some(sig) = tracker.detect_scalp_momentum(volume) {
                                 tracker.pending_signal = Some(sig);
                             }

                             if tracker.pending_signal.is_some() {
                                 tracker.confirmation_count = 0;
                             }
                         }
                    }
                }
            }
        }
    }
}
