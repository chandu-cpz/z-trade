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
            ltq_history: VecDeque::with_capacity(50),
            tick_buf: VecDeque::with_capacity(50),
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

        // Update short tick buffer
        let prev_price = self.tick_buf.back().cloned();
        if prev_price.map(|last| (last - price).abs() > 0.0001).unwrap_or(true) {
            self.tick_buf.push_back(price);

            if self.tick_buf.len() > SCALP_MIN_TICKS {
                self.tick_buf.pop_front();
            }
        }
    }

    pub fn avg_ltq(&self) -> f64 {
        if self.ltq_history.is_empty() { return 0.0; }
        self.ltq_history.iter().sum::<u64>() as f64 / self.ltq_history.len() as f64
    }

    pub fn detect_spoof_trap(&self) -> Option<(PositionSide, Strategy)> {
        if self.tick_buf.len() < SCALP_MIN_TICKS { return None; }
        if self.total_buy_qty == 0 || self.total_sell_qty == 0 { return None; }

        let ratio = self.total_buy_qty as f64 / self.total_sell_qty as f64;
        let slope = self.calculate_slope();

        // 1. Bull Trap (Retail looks at huge Buy wall and buys, but price drops)
        // Action: SHORT (Piggyback institutional spoofers pulling bids)
        if ratio >= SPOOF_OFI_RATIO && slope <= -SPOOF_SLOPE_THRESH {
            return Some((PositionSide::Short, Strategy::SpoofTrap));
        }

        // 2. Bear Trap (Retail looks at huge Sell wall and sells, but price rises)
        // Action: LONG
        if ratio <= (1.0 / SPOOF_OFI_RATIO) && slope >= SPOOF_SLOPE_THRESH {
            return Some((PositionSide::Long, Strategy::SpoofTrap));
        }

        None
    }

    pub fn detect_quiet_accumulation(&self) -> Option<(PositionSide, Strategy)> {
        if self.tick_buf.len() < SCALP_MIN_TICKS { return None; }
        
        // Ensure strictly quiet price history
        let mut min_p = self.tick_buf[0];
        let mut max_p = self.tick_buf[0];
        for &p in &self.tick_buf {
            if p < min_p { min_p = p; }
            if p > max_p { max_p = p; }
        }
        
        if min_p == 0.0 { return None; }
        let variance = (max_p - min_p) / min_p;
        
        if variance > Q_ACCUM_MAX_VARIANCE {
            return None; // Price is too noisy, not a quiet accumulation
        }

        let avg_ltq = self.avg_ltq();
        if avg_ltq == 0.0 { return None; }

        // Breakout trigger: sudden massive institutional absorption hitting the tape
        if self.last_trade_qty as f64 > (avg_ltq * Q_ACCUM_LTQ_MULT) {
            let slope = self.calculate_slope();
            
            if slope > 0.005 {
                return Some((PositionSide::Long, Strategy::QuietAccumulation));
            } else if slope < -0.005 {
                return Some((PositionSide::Short, Strategy::QuietAccumulation));
            }
        }

        None
    }

    pub fn calculate_uas(&self, strategy: Strategy) -> f64 {
        match strategy {
            Strategy::SpoofTrap => {
                // For Spoof Trap, conviction is based on the size of the OFI mismatch and the inverse slope
                let ratio = if self.total_sell_qty > 0 { self.total_buy_qty as f64 / self.total_sell_qty as f64 } else { 1.0 };
                let ratio_score = if ratio > 1.0 { ratio } else { 1.0 / ratio }.min(20.0) * 2.0; // up to 40 pts
                
                let slope_abs = self.calculate_slope().abs();
                let slope_score = (slope_abs * 1000.0).min(40.0); // up to 40 pts
                
                let ltq_score = if self.last_trade_qty > 500 { 20.0 } else { 10.0 }; // up to 20 pts

                (ratio_score + slope_score + ltq_score).clamp(0.0, 100.0)
            },
            Strategy::QuietAccumulation => {
                // For Quiet Accumulation, conviction is based on how tight the variance was and how big the breakout LTQ is
                let mut min_p = self.tick_buf[0];
                let mut max_p = self.tick_buf[0];
                for &p in &self.tick_buf {
                    if p < min_p { min_p = p; }
                    if p > max_p { max_p = p; }
                }
                let variance = if min_p > 0.0 { (max_p - min_p) / min_p } else { 0.001 };
                
                // Tighter variance = higher score (up to 40 pts)
                let compression_score = if variance < 0.0001 { 40.0 } else if variance < 0.0003 { 30.0 } else { 20.0 };
                
                let avg_ltq = self.avg_ltq();
                let ltq_surge = if avg_ltq > 0.0 { self.last_trade_qty as f64 / avg_ltq } else { 1.0 };
                let surge_score = (ltq_surge * 5.0).min(40.0); // up to 40 pts

                let slope_abs = self.calculate_slope().abs();
                let breakout_score = (slope_abs * 1000.0).min(20.0); // up to 20 pts
                
                (compression_score + surge_score + breakout_score).clamp(0.0, 100.0)
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
                             // --- Strategy 1: Spoof Trap (OFT Asymmetry) ---
                             if let Some(sig) = tracker.detect_spoof_trap() {
                                 tracker.pending_signal = Some(sig);
                                 tracker.confirmation_count = 0;
                             }
                             // --- Strategy 2: Quiet Accumulation Breakout ---
                             else if let Some(sig) = tracker.detect_quiet_accumulation() {
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
