use std::collections::VecDeque;

/// O(1) rolling standard deviation and mean accumulator.
#[derive(Debug, Clone)]
pub struct RollingStdDev {
    window_size: usize,
    buf: VecDeque<f64>,
    sum: f64,
    sum_sq: f64,
}

impl RollingStdDev {
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            buf: VecDeque::with_capacity(window_size + 1),
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    pub fn update(&mut self, val: f64) -> (f64, f64) {
        self.buf.push_back(val);
        self.sum += val;
        self.sum_sq += val * val;

        if self.buf.len() > self.window_size {
            if let Some(old) = self.buf.pop_front() {
                self.sum -= old;
                self.sum_sq -= old * old;
            }
        }

        let n = self.buf.len() as f64;
        if n < 2.0 {
            return (val, 0.0);
        }

        let mean = self.sum / n;
        let variance = ((self.sum_sq / n) - (mean * mean)).max(0.0);
        (mean, variance.sqrt())
    }

    pub fn current(&self) -> (f64, f64) {
        let n = self.buf.len() as f64;
        if n == 0.0 {
            return (0.0, 0.0);
        }
        let mean = self.sum / n;
        let variance = ((self.sum_sq / n) - (mean * mean)).max(0.0);
        (mean, variance.sqrt())
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Linear regression slope calculation over a price buffer.
pub fn calculate_slope(buf: &VecDeque<f64>) -> f64 {
    let n = buf.len();
    if n < 5 {
        return 0.0;
    }
    let n_f = n as f64;
    let sum_x: f64 = (0..n).map(|i| i as f64).sum();
    let sum_y: f64 = buf.iter().sum();
    let sum_xy: f64 = buf.iter().enumerate().map(|(i, &p)| (i as f64) * p).sum();
    let sum_xx: f64 = (0..n).map(|i| (i as f64).powi(2)).sum();

    let numerator = n_f * sum_xy - sum_x * sum_y;
    let denominator = n_f * sum_xx - sum_x.powi(2);

    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

/// Wilder's smoothed 14-period RSI indicator.
#[derive(Debug, Clone)]
pub struct Rsi {
    period: f64,
    pub rsi: f64,
    pub avg_gain: f64,
    pub avg_loss: f64,
    prev_price: Option<f64>,
}

impl Default for Rsi {
    fn default() -> Self {
        Self::new(14)
    }
}

impl Rsi {
    pub fn new(period: usize) -> Self {
        Self {
            period: period as f64,
            rsi: 50.0,
            avg_gain: 0.0,
            avg_loss: 0.0,
            prev_price: None,
        }
    }

    pub fn update(&mut self, price: f64) -> f64 {
        if let Some(prev) = self.prev_price {
            let diff = price - prev;
            let gain = if diff > 0.0 { diff } else { 0.0 };
            let loss = if diff < 0.0 { -diff } else { 0.0 };

            let alpha = 1.0 / self.period;
            self.avg_gain = (gain * alpha) + (self.avg_gain * (1.0 - alpha));
            self.avg_loss = (loss * alpha) + (self.avg_loss * (1.0 - alpha));

            if self.avg_loss > 0.0 {
                let rs = self.avg_gain / self.avg_loss;
                self.rsi = 100.0 - (100.0 / (1.0 + rs));
            } else if self.avg_gain > 0.0 {
                self.rsi = 100.0;
            } else {
                self.rsi = 50.0;
            }
        }
        self.prev_price = Some(price);
        self.rsi
    }
}

/// Exponential Moving Average (EMA).
#[derive(Debug, Clone)]
pub struct Ema {
    alpha: f64,
    value: f64,
    initialized: bool,
}

impl Ema {
    pub fn new(period: usize) -> Self {
        Self {
            alpha: 2.0 / (period as f64 + 1.0),
            value: 0.0,
            initialized: false,
        }
    }

    pub fn update(&mut self, price: f64) -> f64 {
        if !self.initialized {
            self.value = price;
            self.initialized = true;
        } else {
            self.value = (price * self.alpha) + (self.value * (1.0 - self.alpha));
        }
        self.value
    }

    pub fn value(&self) -> f64 {
        self.value
    }
}

/// Volume Weighted Average Price (VWAP) accumulator.
#[derive(Default, Debug, Clone)]
pub struct Vwap {
    pub cumulative_pv: f64,
    pub cumulative_vol: u64,
    pub vwap: f64,
}

impl Vwap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, price: f64, volume: u64) -> f64 {
        if volume > 0 {
            self.cumulative_pv += price * (volume as f64);
            self.cumulative_vol += volume;
            if self.cumulative_vol > 0 {
                self.vwap = self.cumulative_pv / (self.cumulative_vol as f64);
            }
        }
        self.vwap
    }

    pub fn reset(&mut self) {
        self.cumulative_pv = 0.0;
        self.cumulative_vol = 0;
        self.vwap = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rolling_std_dev() {
        let mut sd = RollingStdDev::new(5);
        for &p in &[10.0, 10.0, 10.0, 10.0, 10.0] {
            sd.update(p);
        }
        let (mean, std) = sd.current();
        assert!((mean - 10.0).abs() < 1e-6);
        assert!(std.abs() < 1e-6);
    }

    #[test]
    fn test_rsi_bounds() {
        let mut rsi = Rsi::new(14);
        for p in 1..=30 {
            let val = rsi.update(p as f64);
            assert!(val >= 0.0 && val <= 100.0);
        }
        assert!(rsi.rsi > 70.0); // Consistent uptrend should be high RSI
    }

    #[test]
    fn test_vwap() {
        let mut v = Vwap::new();
        v.update(100.0, 10);
        v.update(110.0, 10);
        assert!((v.vwap - 105.0).abs() < 1e-6);
    }
}
