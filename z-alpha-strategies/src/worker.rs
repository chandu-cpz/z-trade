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
    pub _timestamp: DateTime<Local>,
    pub uas: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// STOCK TRACKER (Parallel Version)
// ═══════════════════════════════════════════════════════════════════════════

pub struct StockTracker {
    pub _symbol: String,
    pub price_history: VecDeque<PricePoint>,
    pub current_price: f64,
    pub last_alert: Option<DateTime<Local>>,
    // Signal queued for confirmation (mode is always ScalpMom)
    pub pending_signal: Option<(PositionSide, Strategy)>,
    pub confirmation_count: usize,
    // Order flow tracking
    pub total_buy_qty: u64,   
    pub total_sell_qty: u64,  
    pub best_bid: f64,        
    pub best_ask: f64,        
    pub last_trade_qty: u64,  
    pub ltq_history: VecDeque<u64>, 
    // Short rolling tick buffer
    pub tick_buf: VecDeque<f64>,
    pub tick_timestamps: VecDeque<DateTime<Local>>, // for tick speed
    pub vol_history: VecDeque<u64>,                 // for volume spikes
    pub cum_vol_delta: f64,                         // for order flow divergence
    pub vwap_num: f64,
    pub vwap_den: f64,
    // Order flow imbalance
    pub imbalance: f64,
}

impl StockTracker {
    pub fn new(symbol: String) -> Self {
        Self {
            _symbol: symbol,
            price_history: VecDeque::with_capacity(200),
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
            tick_buf: VecDeque::with_capacity(150),
            tick_timestamps: VecDeque::with_capacity(150),
            vol_history: VecDeque::with_capacity(60),
            cum_vol_delta: 0.0,
            vwap_num: 0.0,
            vwap_den: 0.0,
            imbalance: 0.0,
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

    pub fn update(&mut self, price: f64, _volume: u64, tbq: u64, tsq: u64, bp: f64, sp: f64, ltq: u64, now: DateTime<Local>) {
        self.current_price = price;

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

        // Maintain VWAP
        if _volume > 0 {
            let typ_price = price; // approx
            self.vwap_num += typ_price * (_volume as f64);
            self.vwap_den += _volume as f64;
        }

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

        // Maintain volume history for spikes
        self.vol_history.push_back(_volume);
        if self.vol_history.len() > 60 {
            self.vol_history.pop_front();
        }

        // Update short tick buffer
        let prev_price = self.tick_buf.back().cloned();
        
        // Cum vol delta approx
        if let Some(prev) = prev_price {
            if price > prev { self.cum_vol_delta += _volume as f64; }
            else if price < prev { self.cum_vol_delta -= _volume as f64; }
        }

        if prev_price.map(|last| (last - price).abs() > 0.0001).unwrap_or(true) {
            self.tick_buf.push_back(price);
            self.tick_timestamps.push_back(now);

            // Need larger min ticks for microbreakouts
            if self.tick_buf.len() > MICRO_BREAKOUT_TICKS + 10 {
                self.tick_buf.pop_front();
                self.tick_timestamps.pop_front();
            }
        }
    }

    pub fn detect_master_confluence(&self) -> Option<(PositionSide, Strategy)> {
        if self.tick_buf.len() < 20 || self.vol_history.len() < 20 { return None; }
        
        let slope = self.calculate_slope();
        
        // 1. Order Flow Imbalance
        let ratio = if self.total_sell_qty > 0 { 
            self.total_buy_qty as f64 / self.total_sell_qty as f64 
        } else { 1.0 };
        
        // 2. Volume Spike
        let avg_vol = self.vol_history.iter().take(self.vol_history.len()-1).sum::<u64>() as f64 / (self.vol_history.len() - 1) as f64;
        let curr_vol = *self.vol_history.back().unwrap_or(&0) as f64;
        let vol_mult = if avg_vol > 0.0 { curr_vol / avg_vol } else { 1.0 };

        // LONG CONFLUENCE: Exploding up, massive buy wall, giant volume spike
        if slope > CONFLUENCE_SLOPE && ratio > CONFLUENCE_OFI_RATIO && vol_mult > CONFLUENCE_VOL_MULT {
            return Some((PositionSide::Long, Strategy::AlphaConfluenceLong));
        }

        // SHORT CONFLUENCE: Crashing down, massive sell wall, giant volume spike
        if slope < -CONFLUENCE_SLOPE && ratio < (1.0 / CONFLUENCE_OFI_RATIO) && vol_mult > CONFLUENCE_VOL_MULT {
            return Some((PositionSide::Short, Strategy::AlphaConfluenceShort));
        }

        // LIQUIDITY SWEEP REVERSAL: Sudden drop/spike, hits a huge wall, volume spikes, slope reverses or flattens
        let recent_vol: u64 = self.vol_history.iter().rev().take(5).sum();
        // compare 5-tick rolling sum to 5 * avg_tick
        let avg_5_ticks = avg_vol * 5.0;
        
        if avg_vol > 50.0 && (recent_vol as f64) > (avg_5_ticks * CONFLUENCE_VOL_MULT) && slope.abs() < 0.01 {
            if self.total_buy_qty as f64 > (self.total_sell_qty as f64 * CONFLUENCE_OFI_RATIO) {
                return Some((PositionSide::Long, Strategy::LiquiditySweepReversal));
            } else if self.total_sell_qty as f64 > (self.total_buy_qty as f64 * CONFLUENCE_OFI_RATIO) {
                return Some((PositionSide::Short, Strategy::LiquiditySweepReversal));
            }
        }

        None
    }

    pub fn detect_any_signal(&self) -> Option<(PositionSide, Strategy)> {
        self.detect_master_confluence()
    }

    pub fn calculate_uas(&self, strategy: Strategy) -> f64 {
        match strategy {
            Strategy::AlphaConfluenceLong | Strategy::AlphaConfluenceShort => {
                let slope_abs = self.calculate_slope().abs();
                let slope_score = (slope_abs * 2000.0).min(40.0);
                
                let avg_vol = if self.vol_history.is_empty() { 1.0 } else { self.vol_history.iter().copied().sum::<u64>() as f64 / self.vol_history.len() as f64 };
                let vol_now = self.vol_history.back().copied().unwrap_or(0) as f64;
                let vol_surge = if avg_vol > 0.0 { vol_now / avg_vol } else { 1.0 };
                let vol_score = (vol_surge * 5.0).min(40.0);
                
                let spread = if self.best_bid > 0.0 { (self.best_ask - self.best_bid) / self.best_bid } else { 0.01 };
                // Require very tight spread or deduct massive points
                let spread_score = if spread < 0.0005 { 20.0 } else { -50.0 };

                (slope_score + vol_score + spread_score + 20.0).clamp(0.0, 100.0)
            },
            Strategy::LiquiditySweepReversal => {
                let ratio = if self.total_sell_qty > 0 { self.total_buy_qty as f64 / self.total_sell_qty as f64 } else { 1.0 };
                let ratio_score = if ratio > 1.0 { ratio } else { 1.0 / ratio }.min(10.0) * 4.0;
                let spread = if self.best_bid > 0.0 { (self.best_ask - self.best_bid) / self.best_bid } else { 0.01 };
                let spread_score = if spread < 0.0005 { 20.0 } else { -50.0 };
                (ratio_score + spread_score + 40.0).clamp(0.0, 100.0)
            }
        }
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
                    if let Some((_pending_side, _pending_strategy)) = tracker.pending_signal {
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
                                // FIRE SIGNAL
                                if let Some((side, strategy)) = tracker.pending_signal {
                                    let uas = tracker.calculate_uas(strategy);
                                    let _ = self.signal_tx.send(SignalMsg {
                                        symbol: token.clone(),
                                        price: tracker.current_price,
                                        side,
                                        strategy,
                                        _timestamp: timestamp,
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
                             // --- Evaluate ALL 15 Alpha Strategies ---
                             if let Some(sig) = tracker.detect_any_signal() {
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
