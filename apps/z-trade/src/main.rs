use std::env;
use std::fs;
use std::io::{self, stdout};
use std::time::{Duration, Instant, SystemTime};

use crossterm::{
    cursor::Show,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use dotenvy::dotenv;
use kotak_neo_api::auth::KotakNeo;
use ratatui::{backend::CrosstermBackend, Terminal};
use trade_core::config::Config;
use trade_core::store::SessionStore;
use trade_core::tick::Tick;
use trade_core::types::PositionSide;
use trade_feed::{CsvReplayFeed, KotakFeed, MockFeed};
use trade_sim::PaperSimulationEngine;
use trade_strategy::StrategyEngine;
use trade_tui::{get_watchlist_stocks, render_ui, ActivePane, BottomTab, ProductType, UiState};

const CONFIG_PATH: &str = "configs/default.toml";
const STATE_DIR: &str = "data";
const HISTORICAL_CSV: &str = "data/ticks_2026-02-19.csv";

enum AppFeed {
    Live(KotakFeed),
    Replay(CsvReplayFeed),
    Mock(MockFeed),
}

impl AppFeed {
    async fn next_tick(&mut self) -> Option<Tick> {
        match self {
            AppFeed::Live(f) => f.next_tick().await,
            AppFeed::Replay(f) => {
                tokio::time::sleep(Duration::from_millis(1)).await;
                f.next()
            }
            AppFeed::Mock(f) => {
                tokio::time::sleep(Duration::from_millis(25)).await;
                f.poll_tick()
            }
        }
    }
}

struct HotReloader {
    last_modified: SystemTime,
}

impl HotReloader {
    fn new() -> Self {
        let last_modified = fs::metadata(CONFIG_PATH)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        Self { last_modified }
    }

    fn check_and_reload(
        &mut self,
        sim_engine: &mut PaperSimulationEngine,
        strat_engine: &mut StrategyEngine,
        ui_state: &mut UiState,
    ) {
        if let Ok(meta) = fs::metadata(CONFIG_PATH) {
            if let Ok(mod_time) = meta.modified() {
                if mod_time > self.last_modified {
                    self.last_modified = mod_time;
                    let new_config = Config::load_or_default(CONFIG_PATH);
                    sim_engine.config = new_config.clone();
                    strat_engine.config = new_config;
                    let msg = "⚡ Config reloaded from configs/default.toml";
                    sim_engine.log_message(msg.to_string());
                    ui_state.set_status(msg);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let mut replay_csv: Option<String> = None;
    let mut force_mock = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--replay" if i + 1 < args.len() => {
                replay_csv = Some(args[i + 1].clone());
                i += 2;
            }
            "--mock" => {
                force_mock = true;
                i += 1;
            }
            _ => i += 1,
        }
    }

    // Terminal crash safety panic hook
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        let _ = out.execute(LeaveAlternateScreen);
        let _ = out.execute(Show);
        let _ = disable_raw_mode();
        default_hook(info);
    }));

    let config = Config::load_or_default(CONFIG_PATH);
    let session_state = SessionStore::load_or_init(STATE_DIR, config.portfolio.initial_capital);
    let mut sim_engine = PaperSimulationEngine::from_session(config.clone(), session_state);
    let mut strat_engine = StrategyEngine::new(config.clone());
    let mut ui_state = UiState::new();
    let mut hot_reloader = HotReloader::new();

    // Default top NSE Watchlist symbols
    let default_symbols = vec![
        "RELIANCE", "TCS", "HDFCBANK", "INFY", "ICICIBANK",
        "SBIN", "TATAMOTORS", "ITC", "BHARTIARTL", "KOTAKBANK",
        "LT", "AXISBANK", "MARUTI", "SUNPHARMA", "TITAN",
    ];
    for sym in default_symbols {
        let sym_str = sym.to_string();
        if !sim_engine.watchlist.contains(&sym_str) {
            sim_engine.watchlist.push(sym_str.clone());
        }
        strat_engine.trackers.entry(sym_str.clone()).or_insert_with(|| {
            trade_strategy::StockTracker::new(sym_str, 0)
        });
    }

    // Determine Feed source (Replay vs Mock vs Live Kotak)
    let (mut feed, feed_status_msg) = if let Some(csv_path) = replay_csv {
        match CsvReplayFeed::open(&csv_path) {
            Ok(rf) => (AppFeed::Replay(rf), format!("Replay Feed: {}", csv_path)),
            Err(e) => {
                eprintln!("Warning: Failed to open CSV {}: {}. Falling back to Mock.", csv_path, e);
                (AppFeed::Mock(MockFeed::new()), "Simulated NSE Paper Feed".to_string())
            }
        }
    } else if force_mock {
        (AppFeed::Mock(MockFeed::new()), "Simulated NSE Paper Feed".to_string())
    } else {
        // Try live Kotak Neo login if credentials exist, with graceful fallback
        match try_connect_kotak().await {
            Ok(live_feed) => (AppFeed::Live(live_feed), "Live Kotak Neo SFeed Connected".to_string()),
            Err(err) => {
                let fallback_msg = if fs::metadata(HISTORICAL_CSV).is_ok() {
                    match CsvReplayFeed::open(HISTORICAL_CSV) {
                        Ok(rf) => {
                            (AppFeed::Replay(rf), format!("Offline: Replaying {}", HISTORICAL_CSV))
                        }
                        Err(_) => (AppFeed::Mock(MockFeed::new()), "Offline: Simulated NSE Paper Feed".to_string()),
                    }
                } else {
                    (AppFeed::Mock(MockFeed::new()), "Offline: Simulated NSE Paper Feed".to_string())
                };
                sim_engine.log_message(format!("Kotak login skipped/failed ({}). Using offline feed.", err));
                fallback_msg
            }
        }
    };

    ui_state.set_status(&feed_status_msg);

    // Initialize Terminal Dashboard
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut last_render = Instant::now();
    let render_interval = Duration::from_millis(25); // Smooth ~40 FPS
    let mut last_save = Instant::now();

    // Main Event Loop
    loop {
        hot_reloader.check_and_reload(&mut sim_engine, &mut strat_engine, &mut ui_state);

        // 1. Keyboard & Input Handling
        if event::poll(Duration::from_millis(2))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }

                // If in Search Mode
                if ui_state.is_searching {
                    match key.code {
                        KeyCode::Esc => {
                            ui_state.is_searching = false;
                            ui_state.search_query.clear();
                            continue;
                        }
                        KeyCode::Enter => {
                            ui_state.is_searching = false;
                            let stocks = get_watchlist_stocks(&sim_engine, &ui_state);
                            if !stocks.is_empty() {
                                ui_state.watchlist_selected = 0;
                                ui_state.selected_symbol = stocks[0].clone();
                            }
                            continue;
                        }
                        KeyCode::Backspace => {
                            ui_state.search_query.pop();
                            continue;
                        }
                        KeyCode::Char(c) => {
                            ui_state.search_query.push(c);
                            continue;
                        }
                        _ => continue,
                    }
                }

                // If Order Modal is open
                if ui_state.order_modal.is_open {
                    match key.code {
                        KeyCode::Esc => {
                            ui_state.order_modal.close();
                            continue;
                        }
                        KeyCode::Enter => {
                            let order = ui_state.order_modal.to_pending_order();
                            let sym = order.symbol.clone();
                            let side = order.side;
                            let qty = order.quantity;
                            let ref_price = ui_state.order_modal.price;
                            sim_engine.submit_order(order, ref_price);
                            let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                            ui_state.set_status(format!("Submitted {} order for {} x {}", side, qty, sym));
                            ui_state.order_modal.close();
                            continue;
                        }
                        KeyCode::Char('m') => {
                            ui_state.order_modal.product = ProductType::Mis;
                            continue;
                        }
                        KeyCode::Char('c') => {
                            ui_state.order_modal.product = ProductType::Cnc;
                            continue;
                        }
                        KeyCode::Char('t') => {
                            ui_state.order_modal.toggle_order_type();
                            continue;
                        }
                        KeyCode::Left => {
                            if ui_state.order_modal.quantity > 10 {
                                ui_state.order_modal.quantity -= 10;
                            } else if ui_state.order_modal.quantity > 1 {
                                ui_state.order_modal.quantity -= 1;
                            }
                            continue;
                        }
                        KeyCode::Right => {
                            ui_state.order_modal.quantity += 10;
                            continue;
                        }
                        KeyCode::Up => {
                            ui_state.order_modal.limit_price += 0.50;
                            continue;
                        }
                        KeyCode::Down => {
                            if ui_state.order_modal.limit_price > 0.50 {
                                ui_state.order_modal.limit_price -= 0.50;
                            }
                            continue;
                        }
                        _ => continue,
                    }
                }

                // If Help Modal is open
                if ui_state.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                            ui_state.show_help = false;
                            continue;
                        }
                        _ => {}
                    }
                }

                let current_watchlist = get_watchlist_stocks(&sim_engine, &ui_state);
                let max_rows = match ui_state.active_pane {
                    ActivePane::Watchlist => current_watchlist.len(),
                    ActivePane::BottomTabs => match ui_state.bottom_tab {
                        BottomTab::Positions => sim_engine.positions.len(),
                        BottomTab::Orders => sim_engine.pending_orders.len() + sim_engine.trades.len(),
                        BottomTab::Funds => 0,
                    },
                    ActivePane::StockView => 0,
                };

                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') => break,

                    // Navigation: Next / Prev Row
                    KeyCode::Char('j') | KeyCode::Down => {
                        ui_state.next_row(max_rows);
                        sync_selected_stock(&mut ui_state, &sim_engine);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        ui_state.prev_row();
                        sync_selected_stock(&mut ui_state, &sim_engine);
                    }

                    // Watchlist Selection & Search
                    KeyCode::Enter => {
                        if ui_state.active_pane == ActivePane::Watchlist {
                            sync_selected_stock(&mut ui_state, &sim_engine);
                        }
                    }
                    KeyCode::Char('/') => {
                        ui_state.is_searching = true;
                        ui_state.search_query.clear();
                    }

                    // Pane Navigation
                    KeyCode::Tab | KeyCode::Char('h') | KeyCode::Char('l') => {
                        ui_state.cycle_pane();
                    }

                    // Context-sensitive 1 / 2 / 3 Tab Switchers
                    KeyCode::Char('1') => match ui_state.active_pane {
                        ActivePane::Watchlist => {
                            ui_state.watchlist_tab = 0;
                            ui_state.watchlist_selected = 0;
                            sync_selected_stock(&mut ui_state, &sim_engine);
                        }
                        ActivePane::StockView => ui_state.chart_timeframe = 60,
                        ActivePane::BottomTabs => ui_state.bottom_tab = BottomTab::Positions,
                    },
                    KeyCode::Char('2') => match ui_state.active_pane {
                        ActivePane::Watchlist => {
                            ui_state.watchlist_tab = 1;
                            ui_state.watchlist_selected = 0;
                            sync_selected_stock(&mut ui_state, &sim_engine);
                        }
                        ActivePane::StockView => ui_state.chart_timeframe = 300,
                        ActivePane::BottomTabs => ui_state.bottom_tab = BottomTab::Orders,
                    },
                    KeyCode::Char('3') => match ui_state.active_pane {
                        ActivePane::Watchlist => {
                            ui_state.watchlist_tab = 2;
                            ui_state.watchlist_selected = 0;
                            sync_selected_stock(&mut ui_state, &sim_engine);
                        }
                        ActivePane::StockView => ui_state.chart_timeframe = 900,
                        ActivePane::BottomTabs => ui_state.bottom_tab = BottomTab::Funds,
                    },

                    // Order Cancellation (when on Orders tab)
                    KeyCode::Char('c') => {
                        if ui_state.active_pane == ActivePane::BottomTabs && ui_state.bottom_tab == BottomTab::Orders {
                            if !sim_engine.pending_orders.is_empty() {
                                let idx = ui_state.orders_selected.min(sim_engine.pending_orders.len().saturating_sub(1));
                                let id = sim_engine.pending_orders[idx].id.clone();
                                sim_engine.cancel_order(&id);
                                ui_state.set_status(format!("Cancelled order {}", id));
                                let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                            }
                        }
                    }
                    KeyCode::Char('C') => {
                        if ui_state.active_pane == ActivePane::BottomTabs && ui_state.bottom_tab == BottomTab::Orders {
                            let count = sim_engine.cancel_all_pending();
                            ui_state.set_status(format!("Cancelled all {} pending orders", count));
                            let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                        }
                    }

                    // Trading: Order Slip (b for Buy, s for Sell)
                    KeyCode::Char('b') => {
                        let sym = ui_state.selected_symbol.clone();
                        let ltp = strat_engine.trackers.get(&sym).map(|t| t.current_price).unwrap_or(100.0);
                        ui_state.order_modal.open(sym, PositionSide::Long, ltp);
                    }
                    KeyCode::Char('s') => {
                        let sym = ui_state.selected_symbol.clone();
                        let ltp = strat_engine.trackers.get(&sym).map(|t| t.current_price).unwrap_or(100.0);
                        ui_state.order_modal.open(sym, PositionSide::Short, ltp);
                    }

                    // Trading: Square off selected position
                    KeyCode::Char('x') => {
                        let mut sorted: Vec<String> = sim_engine.positions.keys().cloned().collect();
                        sorted.sort();
                        if let Some(target_sym) = sorted.get(ui_state.positions_selected) {
                            if let Some(pos) = sim_engine.positions.remove(target_sym) {
                                let exit_price = pos.current_price;
                                sim_engine.close_trade(pos, exit_price, "ManualSquareOff");
                                ui_state.set_status(format!("Squared off {}", target_sym));
                                let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                            }
                        }
                    }

                    // Panic: Square off ALL positions
                    KeyCode::Char('X') => {
                        let keys: Vec<String> = sim_engine.positions.keys().cloned().collect();
                        let count = keys.len();
                        for sym in keys {
                            if let Some(pos) = sim_engine.positions.remove(&sym) {
                                let exit_price = pos.current_price;
                                sim_engine.close_trade(pos, exit_price, "PanicExitAll");
                            }
                        }
                        ui_state.set_status(format!("Squared off all {} positions", count));
                        let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                    }

                    // View Toggles
                    KeyCode::Char('d') => {
                        ui_state.show_depth = !ui_state.show_depth;
                    }
                    KeyCode::Char('?') => {
                        ui_state.show_help = !ui_state.show_help;
                    }

                    _ => {}
                }
            }
        }

        // 2. Market Data Ingestion & Tick Processing
        tokio::select! {
            tick_opt = feed.next_tick() => {
                if let Some(tick) = tick_opt {
                    sim_engine.update_tick(&tick);
                    if let Some(sig) = strat_engine.on_tick(&tick) {
                        sim_engine.handle_signal(sig);
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(5)) => {}
        }

        // 3. Periodic Autosave (Every 5 seconds)
        if last_save.elapsed() >= Duration::from_secs(5) {
            let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
            last_save = Instant::now();
        }

        // 4. Render UI
        if last_render.elapsed() >= render_interval {
            terminal.draw(|f| render_ui(f, &sim_engine, &strat_engine, &ui_state))?;
            last_render = Instant::now();
        }
    }

    // Restore Terminal State
    let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show)?;
    terminal.show_cursor()?;

    println!("\n═══════════ Z-TRADE SESSION CLOSED ═══════════");
    println!("  Session State:       {}/session_state.toml", STATE_DIR);
    println!("  Total Realized P&L:  ₹{:.2}", sim_engine.stats.realized_pnl);
    println!("  Total Trades:        {}", sim_engine.stats.trades_count);
    println!("  Win Rate:            {:.1}%\n", sim_engine.stats.win_rate());

    Ok(())
}

fn sync_selected_stock(ui_state: &mut UiState, sim_engine: &PaperSimulationEngine) {
    let stocks = get_watchlist_stocks(sim_engine, ui_state);
    if !stocks.is_empty() {
        let idx = ui_state.watchlist_selected.min(stocks.len().saturating_sub(1));
        ui_state.selected_symbol = stocks[idx].clone();
    }
}

async fn try_connect_kotak() -> Result<KotakFeed, Box<dyn std::error::Error + Send + Sync>> {
    let access_token = env::var("KOTAK_ACCESS_TOKEN")?;
    let mobile_number = env::var("KOTAK_MOBILE_NUMBER")?;
    let ucc = env::var("KOTAK_UCC")?;
    let totp_secret = env::var("KOTAK_TOTP_SECRET")?;
    let mpin = env::var("KOTAK_MPIN")?;

    let mut api = KotakNeo::new();
    // 3-second timeout for login
    tokio::time::timeout(
        Duration::from_secs(3),
        api.login(&access_token, &mobile_number, &ucc, &totp_secret, &mpin),
    )
    .await??;

    let feed = KotakFeed::connect(&api, Some(50)).await?;
    Ok(feed)
}
