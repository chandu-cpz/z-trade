use std::time::Instant;
use crate::order_modal::OrderModalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Watchlist,
    StockView,
    BottomTabs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Positions = 0,
    Orders = 1,
    Funds = 2,
}

pub struct UiState {
    pub active_pane: ActivePane,
    pub bottom_tab: BottomTab,
    
    // Watchlist
    pub watchlist_selected: usize,
    pub watchlist_tab: usize, // 0: NIFTY 50, 1: F&O, 2: HOLDINGS
    pub search_query: String,
    pub is_searching: bool,
    pub selected_symbol: String,
    
    // Bottom tables
    pub positions_selected: usize,
    pub orders_selected: usize,
    
    // View toggles
    pub show_depth: bool,
    pub chart_timeframe: i64, // 60, 300, 900
    pub show_help: bool,
    pub show_debug: bool,
    
    // Modals & messages
    pub order_modal: OrderModalState,
    pub status_message: Option<(String, Instant)>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_pane: ActivePane::Watchlist,
            bottom_tab: BottomTab::Positions,
            watchlist_selected: 0,
            watchlist_tab: 0,
            search_query: String::new(),
            is_searching: false,
            selected_symbol: "RELIANCE".to_string(),
            positions_selected: 0,
            orders_selected: 0,
            show_depth: true,
            chart_timeframe: 60,
            show_help: false,
            show_debug: false,
            order_modal: OrderModalState::default(),
            status_message: None,
        }
    }
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn cycle_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Watchlist => ActivePane::StockView,
            ActivePane::StockView => ActivePane::BottomTabs,
            ActivePane::BottomTabs => ActivePane::Watchlist,
        };
    }

    pub fn next_row(&mut self, max_rows: usize) {
        if max_rows == 0 {
            return;
        }
        match self.active_pane {
            ActivePane::Watchlist => {
                if self.watchlist_selected + 1 < max_rows {
                    self.watchlist_selected += 1;
                }
            }
            ActivePane::BottomTabs => {
                match self.bottom_tab {
                    BottomTab::Positions => {
                        if self.positions_selected + 1 < max_rows {
                            self.positions_selected += 1;
                        }
                    }
                    BottomTab::Orders => {
                        if self.orders_selected + 1 < max_rows {
                            self.orders_selected += 1;
                        }
                    }
                    BottomTab::Funds => {}
                }
            }
            ActivePane::StockView => {}
        }
    }

    pub fn prev_row(&mut self) {
        match self.active_pane {
            ActivePane::Watchlist => {
                if self.watchlist_selected > 0 {
                    self.watchlist_selected -= 1;
                }
            }
            ActivePane::BottomTabs => {
                match self.bottom_tab {
                    BottomTab::Positions => {
                        if self.positions_selected > 0 {
                            self.positions_selected -= 1;
                        }
                    }
                    BottomTab::Orders => {
                        if self.orders_selected > 0 {
                            self.orders_selected -= 1;
                        }
                    }
                    BottomTab::Funds => {}
                }
            }
            ActivePane::StockView => {}
        }
    }
}
