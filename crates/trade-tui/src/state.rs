use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Dashboard = 0,
    Chart = 1,
    Signals = 2,
    Analytics = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Holdings,
    Tradebook,
    Signals,
    Watchlist,
    Chart,
}

pub struct UiState {
    pub active_tab: ActiveTab,
    pub active_pane: ActivePane,
    pub holdings_selected: usize,
    pub tradebook_selected: usize,
    pub signals_selected: usize,
    pub watchlist_selected: usize,
    pub selected_symbol: Option<String>,
    pub order_modal: crate::order_modal::OrderModalState,
    pub show_help: bool,
    pub show_debug: bool,
    pub status_message: Option<(String, Instant)>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_tab: ActiveTab::Dashboard,
            active_pane: ActivePane::Holdings,
            holdings_selected: 0,
            tradebook_selected: 0,
            signals_selected: 0,
            watchlist_selected: 0,
            selected_symbol: None,
            order_modal: crate::order_modal::OrderModalState::default(),
            show_help: false,
            show_debug: false,
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
            ActivePane::Holdings => ActivePane::Tradebook,
            ActivePane::Tradebook => ActivePane::Watchlist,
            ActivePane::Watchlist => ActivePane::Holdings,
            ActivePane::Signals => ActivePane::Holdings,
            ActivePane::Chart => ActivePane::Holdings,
        };
    }

    pub fn next_row(&mut self, max_rows: usize) {
        if max_rows == 0 {
            return;
        }
        match self.active_pane {
            ActivePane::Holdings => {
                if self.holdings_selected + 1 < max_rows {
                    self.holdings_selected += 1;
                }
            }
            ActivePane::Tradebook => {
                if self.tradebook_selected + 1 < max_rows {
                    self.tradebook_selected += 1;
                }
            }
            ActivePane::Signals => {
                if self.signals_selected + 1 < max_rows {
                    self.signals_selected += 1;
                }
            }
            ActivePane::Watchlist => {
                if self.watchlist_selected + 1 < max_rows {
                    self.watchlist_selected += 1;
                }
            }
            ActivePane::Chart => {}
        }
    }

    pub fn prev_row(&mut self) {
        match self.active_pane {
            ActivePane::Holdings => {
                if self.holdings_selected > 0 {
                    self.holdings_selected -= 1;
                }
            }
            ActivePane::Tradebook => {
                if self.tradebook_selected > 0 {
                    self.tradebook_selected -= 1;
                }
            }
            ActivePane::Signals => {
                if self.signals_selected > 0 {
                    self.signals_selected -= 1;
                }
            }
            ActivePane::Watchlist => {
                if self.watchlist_selected > 0 {
                    self.watchlist_selected -= 1;
                }
            }
            ActivePane::Chart => {}
        }
    }

    pub fn jump_to_top(&mut self) {
        match self.active_pane {
            ActivePane::Holdings => self.holdings_selected = 0,
            ActivePane::Tradebook => self.tradebook_selected = 0,
            ActivePane::Signals => self.signals_selected = 0,
            ActivePane::Watchlist => self.watchlist_selected = 0,
            ActivePane::Chart => {}
        }
    }

    pub fn jump_to_bottom(&mut self, max_rows: usize) {
        if max_rows == 0 {
            return;
        }
        match self.active_pane {
            ActivePane::Holdings => self.holdings_selected = max_rows.saturating_sub(1),
            ActivePane::Tradebook => self.tradebook_selected = max_rows.saturating_sub(1),
            ActivePane::Signals => self.signals_selected = max_rows.saturating_sub(1),
            ActivePane::Watchlist => self.watchlist_selected = max_rows.saturating_sub(1),
            ActivePane::Chart => {}
        }
    }
}
