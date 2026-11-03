use std::collections::VecDeque;

/// Calculates the ordinary least squares (OLS) linear regression slope of a price buffer.
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

/// Calculates standard deviation over a rolling slice.
pub fn calculate_std_dev(buf: &VecDeque<f64>) -> f64 {
    if buf.is_empty() {
        return 0.0;
    }
    let n = buf.len() as f64;
    let mean = buf.iter().sum::<f64>() / n;
    let variance = buf.iter().map(|&p| (p - mean).powi(2)).sum::<f64>() / n;
    variance.sqrt()
}

/// Computes Order Book Imbalance ratio in range [-1.0, 1.0].
/// Positive indicates bid-heavy, negative indicates ask-heavy.
pub fn calculate_order_book_imbalance(total_buy_qty: u64, total_sell_qty: u64) -> f64 {
    let total = (total_buy_qty + total_sell_qty) as f64;
    if total > 0.0 {
        (total_buy_qty as f64 - total_sell_qty as f64) / total
    } else {
        0.0
    }
}

/// Helper struct to update Wilder's smoothed RSI.
#[derive(Debug, Clone)]
pub struct RsiState {
    pub rsi: f64,
    pub avg_gain: f64,
    pub avg_loss: f64,
}

impl Default for RsiState {
    fn default() -> Self {
        Self {
            rsi: 50.0,
            avg_gain: 0.0,
            avg_loss: 0.0,
        }
    }
}

impl RsiState {
    pub fn update(&mut self, prev_price: f64, current_price: f64) -> f64 {
        let diff = current_price - prev_price;
        let gain = if diff > 0.0 { diff } else { 0.0 };
        let loss = if diff < 0.0 { -diff } else { 0.0 };

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

        self.rsi
    }
}
