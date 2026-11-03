use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::position::Position;
use crate::trade::{Trade, TradingStats};
use crate::types::PositionSide;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOrder {
    pub id: String,
    pub symbol: String,
    pub token: u32,
    pub side: PositionSide,
    pub order_type: String, // "MARKET", "LIMIT", "STOP_LOSS"
    pub quantity: u32,
    pub limit_price: Option<f64>,
    pub trigger_price: Option<f64>,
    pub stop_loss: Option<f64>,
    pub target: Option<f64>,
    pub placed_at: DateTime<Local>,
    pub origin: String, // "MANUAL" or "ALGO"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_date: String, // "YYYY-MM-DD"
    pub initial_capital: f64,
    pub free_capital: f64,
    pub locked_capital: f64,
    pub positions: HashMap<String, Position>,
    pub pending_orders: Vec<PendingOrder>,
    pub trades: Vec<Trade>,
    pub stats: TradingStats,
    pub watchlist: Vec<String>,
    pub algo_enabled: bool,
    pub last_saved_at: DateTime<Local>,
}

pub struct SessionStore;

impl SessionStore {
    fn state_file_path(data_dir: &Path) -> PathBuf {
        data_dir.join("session_state.toml")
    }

    fn history_dir_path(data_dir: &Path) -> PathBuf {
        data_dir.join("history")
    }

    /// Loads active intraday session state or initializes a new day session.
    pub fn load_or_init(data_dir: impl AsRef<Path>, default_initial_capital: f64) -> SessionState {
        let dir = data_dir.as_ref();
        let _ = fs::create_dir_all(dir);
        let _ = fs::create_dir_all(Self::history_dir_path(dir));

        let file_path = Self::state_file_path(dir);
        let today = Local::now().format("%Y-%m-%d").to_string();

        if file_path.exists() {
            if let Ok(content) = fs::read_to_string(&file_path) {
                if let Ok(mut state) = toml::from_str::<SessionState>(&content) {
                    // Check if state is from today
                    if state.session_date == today {
                        return state;
                    }

                    // Day rollover: archive yesterday's state
                    let archive_name = format!("session_{}.toml", state.session_date);
                    let archive_path = Self::history_dir_path(dir).join(archive_name);
                    let _ = fs::write(&archive_path, &content);

                    // Carry forward equity
                    let carried_capital = state.free_capital + state.locked_capital;
                    state.session_date = today;
                    state.initial_capital = carried_capital;
                    state.free_capital = carried_capital;
                    state.locked_capital = 0.0;
                    state.positions.clear();
                    state.pending_orders.clear();
                    state.trades.clear();
                    state.stats = TradingStats::new();
                    state.last_saved_at = Local::now();

                    let _ = Self::save(dir, &state);
                    return state;
                }
            }
        }

        // Initialize clean state
        let default_watchlist = vec![
            "RELIANCE-EQ".to_string(),
            "TCS-EQ".to_string(),
            "HDFCBANK-EQ".to_string(),
            "INFY-EQ".to_string(),
            "ICICIBANK-EQ".to_string(),
            "SBIN-EQ".to_string(),
            "BHARTIARTL-EQ".to_string(),
            "LT-EQ".to_string(),
        ];

        let state = SessionState {
            session_date: today,
            initial_capital: default_initial_capital,
            free_capital: default_initial_capital,
            locked_capital: 0.0,
            positions: HashMap::new(),
            pending_orders: Vec::new(),
            trades: Vec::new(),
            stats: TradingStats::new(),
            watchlist: default_watchlist,
            algo_enabled: true,
            last_saved_at: Local::now(),
        };

        let _ = Self::save(dir, &state);
        state
    }

    /// Atomically persists session state to disk using a write-ahead temp file and atomic rename.
    pub fn save(data_dir: impl AsRef<Path>, state: &SessionState) -> std::io::Result<()> {
        let dir = data_dir.as_ref();
        let target_path = Self::state_file_path(dir);
        let temp_path = dir.join("session_state.toml.tmp");

        let toml_str = toml::to_string_pretty(state).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("TOML serialization error: {}", e))
        })?;

        fs::write(&temp_path, toml_str)?;
        fs::rename(&temp_path, &target_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_store_init_and_save() {
        let temp_dir = std::env::temp_dir().join(format!("test_session_toml_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let initial_capital = 500_000.0;
        let mut state = SessionStore::load_or_init(&temp_dir, initial_capital);
        assert_eq!(state.initial_capital, initial_capital);
        assert_eq!(state.free_capital, initial_capital);
        assert!(state.positions.is_empty());
        assert!(state.pending_orders.is_empty());

        // Modify state
        state.free_capital = 450_000.0;
        state.locked_capital = 50_000.0;
        state.pending_orders.push(PendingOrder {
            id: "TEST-1".to_string(),
            symbol: "TCS-EQ".to_string(),
            token: 11536,
            side: PositionSide::Long,
            order_type: "LIMIT".to_string(),
            quantity: 10,
            limit_price: Some(3000.0),
            trigger_price: None,
            stop_loss: Some(2970.0),
            target: Some(3060.0),
            placed_at: Local::now(),
            origin: "MANUAL".to_string(),
        });

        // Save TOML
        SessionStore::save(&temp_dir, &state).expect("Save should succeed");
        assert!(temp_dir.join("session_state.toml").exists());

        // Reload from TOML
        let loaded = SessionStore::load_or_init(&temp_dir, initial_capital);
        assert_eq!(loaded.free_capital, 450_000.0);
        assert_eq!(loaded.locked_capital, 50_000.0);
        assert_eq!(loaded.pending_orders.len(), 1);
        assert_eq!(loaded.pending_orders[0].symbol, "TCS-EQ");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
