use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioConfig {
    #[serde(default = "default_initial_capital")]
    pub initial_capital: f64,
    #[serde(default = "default_max_positions")]
    pub max_positions: usize,
    #[serde(default = "default_max_loss")]
    pub max_loss: f64,
    #[serde(default = "default_max_price")]
    pub max_price: f64,
    #[serde(default = "default_min_capital_per_trade")]
    pub min_capital_per_trade: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default = "default_stop_loss_pct")]
    pub stop_loss_pct: f64,
    #[serde(default = "default_target_mult")]
    pub target_mult: f64,
    #[serde(default = "default_trail_stop_trigger_pct")]
    pub trail_stop_trigger_pct: f64,
    #[serde(default = "default_trail_stop_back_pct")]
    pub trail_stop_back_pct: f64,
    #[serde(default = "default_emergency_pct")]
    pub emergency_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionCostConfig {
    #[serde(default = "default_slippage_pct")]
    pub simulated_slippage_pct: f64,
    #[serde(default = "default_brokerage")]
    pub simulated_brokerage_per_order: f64,
    #[serde(default = "default_stt_pct")]
    pub simulated_stt_pct: f64,
    #[serde(default = "default_turnover_pct")]
    pub simulated_turnover_charges_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default = "default_alert_cooldown")]
    pub alert_cooldown_secs: i64,
    #[serde(default = "default_loss_cooldown")]
    pub loss_cooldown_secs: i64,
    #[serde(default = "default_max_spread_pct")]
    pub max_spread_pct: f64,
    #[serde(default = "default_session_start_hour")]
    pub session_start_hour: u32,
    #[serde(default = "default_session_start_min")]
    pub session_start_min: u32,
    #[serde(default = "default_trend_window")]
    pub trend_window_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    #[serde(default = "default_min_ticks")]
    pub min_ticks: usize,
    #[serde(default = "default_confirmation_ticks")]
    pub confirmation_ticks: usize,
    #[serde(default = "default_consec_ticks")]
    pub consec_ticks: usize,
    #[serde(default = "default_vol_burst_mult")]
    pub vol_burst_mult: f64,
    #[serde(default = "default_ltq_burst_mult")]
    pub ltq_burst_mult: f64,
    #[serde(default = "default_accel_thresh")]
    pub accel_thresh: f64,
    #[serde(default = "default_ofi_imbalance_ratio")]
    pub ofi_imbalance_ratio: f64,
    #[serde(default = "default_orb_window_mins")]
    pub orb_window_mins: i64,
    #[serde(default = "default_vwap_stdev_mult")]
    pub vwap_stdev_mult: f64,
    #[serde(default = "default_liq_sweep_tick_pierce")]
    pub liq_sweep_tick_pierce: f64,
    #[serde(default)]
    pub invert_strategy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpulseScoringConfig {
    #[serde(default = "default_ies_base")]
    pub ies_base_threshold: f64,
    #[serde(default = "default_ies_max")]
    pub ies_max_threshold: f64,
    #[serde(default = "default_ies_swap")]
    pub ies_swap_advantage: f64,
    #[serde(default = "default_ies_window")]
    pub ies_window_ticks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    #[serde(default = "default_workers")]
    pub num_workers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub portfolio: PortfolioConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub transaction_costs: TransactionCostConfig,
    #[serde(default)]
    pub filters: FilterConfig,
    #[serde(default)]
    pub strategy: StrategyConfig,
    #[serde(default)]
    pub impulse_scoring: ImpulseScoringConfig,
    #[serde(default)]
    pub engine: EngineConfig,
}

// Default helper functions
fn default_initial_capital() -> f64 { 20000.0 }
fn default_max_positions() -> usize { 25 }
fn default_max_loss() -> f64 { 3000.0 }
fn default_max_price() -> f64 { 5000.0 }
fn default_min_capital_per_trade() -> f64 { 100.0 }
fn default_stop_loss_pct() -> f64 { 0.15 }
fn default_target_mult() -> f64 { 2.0 }
fn default_trail_stop_trigger_pct() -> f64 { 0.30 }
fn default_trail_stop_back_pct() -> f64 { 0.15 }
fn default_emergency_pct() -> f64 { -1.20 }
fn default_slippage_pct() -> f64 { 0.02 }
fn default_brokerage() -> f64 { 20.0 }
fn default_stt_pct() -> f64 { 0.025 }
fn default_turnover_pct() -> f64 { 0.00345 }
fn default_alert_cooldown() -> i64 { 30 }
fn default_loss_cooldown() -> i64 { 600 }
fn default_max_spread_pct() -> f64 { 0.20 }
fn default_session_start_hour() -> u32 { 9 }
fn default_session_start_min() -> u32 { 15 }
fn default_trend_window() -> i64 { 300 }
fn default_min_ticks() -> usize { 50 }
fn default_confirmation_ticks() -> usize { 1 }
fn default_consec_ticks() -> usize { 3 }
fn default_vol_burst_mult() -> f64 { 5.0 }
fn default_ltq_burst_mult() -> f64 { 4.0 }
fn default_accel_thresh() -> f64 { 0.10 }
fn default_ofi_imbalance_ratio() -> f64 { 3.0 }
fn default_orb_window_mins() -> i64 { 5 }
fn default_vwap_stdev_mult() -> f64 { 3.0 }
fn default_liq_sweep_tick_pierce() -> f64 { 0.05 }
fn default_ies_base() -> f64 { 60.0 }
fn default_ies_max() -> f64 { 85.0 }
fn default_ies_swap() -> f64 { 20.0 }
fn default_ies_window() -> usize { 10 }
fn default_workers() -> usize { 8 }

impl Default for PortfolioConfig {
    fn default() -> Self {
        Self {
            initial_capital: default_initial_capital(),
            max_positions: default_max_positions(),
            max_loss: default_max_loss(),
            max_price: default_max_price(),
            min_capital_per_trade: default_min_capital_per_trade(),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            stop_loss_pct: default_stop_loss_pct(),
            target_mult: default_target_mult(),
            trail_stop_trigger_pct: default_trail_stop_trigger_pct(),
            trail_stop_back_pct: default_trail_stop_back_pct(),
            emergency_pct: default_emergency_pct(),
        }
    }
}

impl Default for TransactionCostConfig {
    fn default() -> Self {
        Self {
            simulated_slippage_pct: default_slippage_pct(),
            simulated_brokerage_per_order: default_brokerage(),
            simulated_stt_pct: default_stt_pct(),
            simulated_turnover_charges_pct: default_turnover_pct(),
        }
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            alert_cooldown_secs: default_alert_cooldown(),
            loss_cooldown_secs: default_loss_cooldown(),
            max_spread_pct: default_max_spread_pct(),
            session_start_hour: default_session_start_hour(),
            session_start_min: default_session_start_min(),
            trend_window_secs: default_trend_window(),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            min_ticks: default_min_ticks(),
            confirmation_ticks: default_confirmation_ticks(),
            consec_ticks: default_consec_ticks(),
            vol_burst_mult: default_vol_burst_mult(),
            ltq_burst_mult: default_ltq_burst_mult(),
            accel_thresh: default_accel_thresh(),
            ofi_imbalance_ratio: default_ofi_imbalance_ratio(),
            orb_window_mins: default_orb_window_mins(),
            vwap_stdev_mult: default_vwap_stdev_mult(),
            liq_sweep_tick_pierce: default_liq_sweep_tick_pierce(),
            invert_strategy: false,
        }
    }
}

impl Default for ImpulseScoringConfig {
    fn default() -> Self {
        Self {
            ies_base_threshold: default_ies_base(),
            ies_max_threshold: default_ies_max(),
            ies_swap_advantage: default_ies_swap(),
            ies_window_ticks: default_ies_window(),
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            num_workers: default_workers(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            portfolio: PortfolioConfig::default(),
            execution: ExecutionConfig::default(),
            transaction_costs: TransactionCostConfig::default(),
            filters: FilterConfig::default(),
            strategy: StrategyConfig::default(),
            impulse_scoring: ImpulseScoringConfig::default(),
            engine: EngineConfig::default(),
        }
    }
}

impl Config {
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&content)?;
        Ok(cfg)
    }

    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        Self::load_from_file(path).unwrap_or_default()
    }
}
