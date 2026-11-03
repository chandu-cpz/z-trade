/// Trading and risk parameters for the paper trading engine.
#[derive(Debug, Clone)]
pub struct Config {
    // Portfolio & Risk Limits
    pub initial_capital: f64,
    pub max_positions: usize,
    pub max_loss: f64,
    pub max_price: f64,
    pub min_capital_per_trade: f64,

    // Execution & Stops (Percentages)
    pub stop_loss_pct: f64,
    pub target_mult: f64,
    pub trail_stop_trigger_pct: f64,
    pub trail_stop_back_pct: f64,
    pub emergency_pct: f64,

    // Filters & Cooldowns
    pub alert_cooldown_secs: i64,
    pub loss_cooldown_secs: i64,
    pub max_spread_pct: f64,
    pub session_start_hour: u32,
    pub session_start_min: u32,
    pub trend_window_secs: i64,

    // Strategy Parameters
    pub min_ticks: usize,
    pub confirmation_ticks: usize,
    pub consec_ticks: usize,
    pub vol_burst_mult: f64,
    pub ltq_burst_mult: f64,
    pub accel_thresh: f64,
    pub ofi_imbalance_ratio: f64,
    pub orb_window_mins: i64,
    pub vwap_stdev_mult: f64,
    pub liq_sweep_tick_pierce: f64,

    // Impulse Energy Scoring (IES / UAS)
    pub ies_base_threshold: f64,
    pub ies_max_threshold: f64,
    pub ies_swap_advantage: f64,
    pub ies_window_ticks: usize,

    // Invert strategy signals (Experimental)
    pub invert_strategy: bool,

    // Worker threads
    pub num_workers: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            initial_capital: 20000.0,
            max_positions: 25,
            max_loss: 3000.0,
            max_price: 5000.0,
            min_capital_per_trade: 100.0,

            // Tight scalp stops tuned in z-folder-improved
            stop_loss_pct: 0.15,
            target_mult: 2.0,
            trail_stop_trigger_pct: 0.30,
            trail_stop_back_pct: 0.15,
            emergency_pct: -1.20,

            alert_cooldown_secs: 30,
            loss_cooldown_secs: 600,
            max_spread_pct: 0.20,
            session_start_hour: 9,
            session_start_min: 15,
            trend_window_secs: 300,

            min_ticks: 50,
            confirmation_ticks: 1,
            consec_ticks: 3,
            vol_burst_mult: 5.0,
            ltq_burst_mult: 4.0,
            accel_thresh: 0.10,
            ofi_imbalance_ratio: 3.0,
            orb_window_mins: 5,
            vwap_stdev_mult: 3.0,
            liq_sweep_tick_pierce: 0.05,

            ies_base_threshold: 60.0,
            ies_max_threshold: 85.0,
            ies_swap_advantage: 20.0,
            ies_window_ticks: 10,

            invert_strategy: false,
            num_workers: 8,
        }
    }
}
