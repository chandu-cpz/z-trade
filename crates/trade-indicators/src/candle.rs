use std::collections::VecDeque;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OhlcvBar {
    pub timestamp: DateTime<Local>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
    pub trades: u64,
    pub vwap: f64,
}

impl OhlcvBar {
    pub fn is_bullish(&self) -> bool {
        self.close >= self.open
    }

    pub fn change_pct(&self) -> f64 {
        if self.open > 0.0 {
            ((self.close - self.open) / self.open) * 100.0
        } else {
            0.0
        }
    }
}

/// Aggregates continuous tick stream into synchronized wall-clock candlestick bars.
#[derive(Debug, Clone)]
pub struct BarAggregator {
    pub timeframe_secs: i64,
    pub current_bar: Option<OhlcvBar>,
    pub completed_bars: VecDeque<OhlcvBar>,
    pub max_bars: usize,
    cumulative_pv: f64,
    cumulative_vol: u64,
}

impl BarAggregator {
    pub fn new(timeframe_secs: i64, max_bars: usize) -> Self {
        Self {
            timeframe_secs,
            current_bar: None,
            completed_bars: VecDeque::with_capacity(max_bars + 1),
            max_bars,
            cumulative_pv: 0.0,
            cumulative_vol: 0,
        }
    }

    pub fn update(&mut self, price: f64, volume: u64, ts: DateTime<Local>) -> Option<OhlcvBar> {
        let ts_sec = ts.timestamp();
        let period_start = ts_sec - (ts_sec % self.timeframe_secs);
        let period_time = DateTime::from_timestamp(period_start, 0)
            .map(|dt: DateTime<Utc>| dt.with_timezone(&Local))
            .unwrap_or(ts);

        self.cumulative_pv += price * (volume as f64);
        self.cumulative_vol += volume;
        let vwap = if self.cumulative_vol > 0 {
            self.cumulative_pv / (self.cumulative_vol as f64)
        } else {
            price
        };

        if let Some(ref mut bar) = self.current_bar {
            if bar.timestamp == period_time {
                // Update active candle
                bar.close = price;
                if price > bar.high {
                    bar.high = price;
                }
                if price < bar.low {
                    bar.low = price;
                }
                bar.volume += volume;
                bar.trades += 1;
                bar.vwap = vwap;
                return None;
            } else {
                // Candle boundary crossed: push completed bar
                let finalized = *bar;
                self.completed_bars.push_back(finalized);
                if self.completed_bars.len() > self.max_bars {
                    self.completed_bars.pop_front();
                }

                *bar = OhlcvBar {
                    timestamp: period_time,
                    open: price,
                    high: price,
                    low: price,
                    close: price,
                    volume,
                    trades: 1,
                    vwap,
                };
                return Some(finalized);
            }
        } else {
            self.current_bar = Some(OhlcvBar {
                timestamp: period_time,
                open: price,
                high: price,
                low: price,
                close: price,
                volume,
                trades: 1,
                vwap,
            });
            None
        }
    }

    pub fn all_bars(&self) -> Vec<OhlcvBar> {
        let mut bars: Vec<OhlcvBar> = self.completed_bars.iter().copied().collect();
        if let Some(active) = self.current_bar {
            bars.push(active);
        }
        bars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_aggregator() {
        let mut agg = BarAggregator::new(60, 100); // 1-minute bars
        let base_time = DateTime::from_timestamp(1700000000, 0).unwrap().with_timezone(&Local);

        // Tick 1 in bar 1
        let finalized1 = agg.update(100.0, 10, base_time);
        assert!(finalized1.is_none());

        // Tick 2 in bar 1 (high)
        let finalized2 = agg.update(105.0, 20, base_time);
        assert!(finalized2.is_none());

        // Tick 3 in bar 1 (low)
        let finalized3 = agg.update(98.0, 15, base_time);
        assert!(finalized3.is_none());

        let active = agg.current_bar.unwrap();
        assert_eq!(active.open, 100.0);
        assert_eq!(active.high, 105.0);
        assert_eq!(active.low, 98.0);
        assert_eq!(active.close, 98.0);
        assert_eq!(active.volume, 45);

        // Tick 4 in bar 2 (next minute)
        let next_min = DateTime::from_timestamp(1700000065, 0).unwrap().with_timezone(&Local);
        let finalized4 = agg.update(102.0, 30, next_min);
        assert!(finalized4.is_some());
        let bar1 = finalized4.unwrap();
        assert_eq!(bar1.open, 100.0);
        assert_eq!(bar1.high, 105.0);
        assert_eq!(bar1.low, 98.0);
        assert_eq!(bar1.close, 98.0);

        let all = agg.all_bars();
        assert_eq!(all.len(), 2);
    }
}
