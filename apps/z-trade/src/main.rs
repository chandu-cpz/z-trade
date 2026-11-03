use std::env;
use std::fs;
use std::io::{self, stdout};
use std::time::{Duration, Instant, SystemTime};

use chrono::Timelike;
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
use trade_core::types::PositionSide;
use trade_feed::KotakFeed;
use trade_sim::PaperSimulationEngine;
use trade_strategy::StrategyEngine;
use trade_tui::{render_ui, ActivePane, ActiveTab, UiState};

const CONFIG_PATH: &str = "configs/default.toml";
const STATE_DIR: &str = "data";

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
                    let msg = "⚡ HOT RELOAD: Strategies & Risk updated from configs/default.toml";
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
    env_logger::init();

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

    // 0. Across-the-Day Session Persistence & Crash Recovery
    let session_state = SessionStore::load_or_init(STATE_DIR, config.portfolio.initial_capital);
    let mut sim_engine = PaperSimulationEngine::from_session(config.clone(), session_state);
    let mut strat_engine = StrategyEngine::new(config.clone());
    let mut ui_state = UiState::new();
    let mut hot_reloader = HotReloader::new();

    let restore_msg = format!(
        "💾 Session Restored: ₹{:.2} cash, {} holdings, {} pending orders",
        sim_engine.free_capital(),
        sim_engine.positions.len(),
        sim_engine.pending_orders.len()
    );
    sim_engine.log_message(restore_msg.clone());
    ui_state.set_status(restore_msg);

    // 1. Authenticate with Kotak Neo API
    let access_token = env::var("KOTAK_ACCESS_TOKEN").expect("KOTAK_ACCESS_TOKEN missing in .env");
    let mobile_number = env::var("KOTAK_MOBILE_NUMBER").expect("KOTAK_MOBILE_NUMBER missing in .env");
    let ucc = env::var("KOTAK_UCC").expect("KOTAK_UCC missing in .env");
    let totp_secret = env::var("KOTAK_TOTP_SECRET").expect("KOTAK_TOTP_SECRET missing in .env");
    let mpin = env::var("KOTAK_MPIN").expect("KOTAK_MPIN missing in .env");

    let mut api = KotakNeo::new();
    api.login(&access_token, &mobile_number, &ucc, &totp_secret, &mpin).await?;
    sim_engine.log_message("Authenticated with Kotak Neo API.".to_string());
    ui_state.set_status("Connected to Kotak Neo SFeed");

    // 2. Connect Market Data Feed
    let mut feed = KotakFeed::connect(&api, None).await?;
    sim_engine.log_message("Subscribed to NSE Cash equity universe via SFeed WebSocket.".to_string());

    // 3. Initialize Terminal Dashboard
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut last_render = Instant::now();
    let render_interval = Duration::from_millis(33); // ~30 FPS
    let mut last_save = Instant::now();

    // 4. Main Event Loop
    loop {
        // A. Check for hot-reloading strategy and risk configs
        hot_reloader.check_and_reload(&mut sim_engine, &mut strat_engine, &mut ui_state);

        // B. Handle Vim & Keyboard Controls
        if event::poll(Duration::from_millis(1))? {
            if let Event::Key(key) = event::read()? {
                // If order modal is visible, direct keys to modal
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
                        KeyCode::Char('t') => {
                            ui_state.order_modal.order_type = if ui_state.order_modal.order_type == "MARKET" {
                                "LIMIT".to_string()
                            } else {
                                "MARKET".to_string()
                            };
                            continue;
                        }
                        KeyCode::Left | KeyCode::Down => {
                            if ui_state.order_modal.quantity > 10 {
                                ui_state.order_modal.quantity -= 10;
                            } else if ui_state.order_modal.quantity > 1 {
                                ui_state.order_modal.quantity -= 1;
                            }
                            continue;
                        }
                        KeyCode::Right | KeyCode::Up => {
                            ui_state.order_modal.quantity += 10;
                            continue;
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            ui_state.order_modal.limit_price += 0.5;
                            continue;
                        }
                        KeyCode::Char('-') | KeyCode::Char('_') => {
                            if ui_state.order_modal.limit_price > 0.5 {
                                ui_state.order_modal.limit_price -= 0.5;
                            }
                            continue;
                        }
                        _ => continue,
                    }
                }

                // If help modal is visible, Esc or ? closes it
                if ui_state.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                            ui_state.show_help = false;
                            continue;
                        }
                        _ => {}
                    }
                }

                // If debug drawer is visible, d or Esc closes it
                if ui_state.show_debug {
                    match key.code {
                        KeyCode::Char('d') | KeyCode::Esc => {
                            ui_state.show_debug = false;
                            continue;
                        }
                        _ => {}
                    }
                }

                let max_rows = match ui_state.active_pane {
                    ActivePane::Watchlist => strat_engine.trackers.len(),
                    ActivePane::Holdings => sim_engine.positions.len(),
                    ActivePane::Tradebook => sim_engine.trades.len(),
                    ActivePane::Signals => sim_engine.signal_feed.len(),
                    ActivePane::Chart => 0,
                };

                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') | KeyCode::Esc => break,

                    // Navigation: Next Row
                    KeyCode::Char('j') | KeyCode::Down => {
                        ui_state.next_row(max_rows);
                        update_selected_symbol(&mut ui_state, &sim_engine, &strat_engine);
                    }
                    // Navigation: Prev Row
                    KeyCode::Char('k') | KeyCode::Up => {
                        ui_state.prev_row();
                        update_selected_symbol(&mut ui_state, &sim_engine, &strat_engine);
                    }
                    // Navigation: Top
                    KeyCode::Char('g') => {
                        ui_state.jump_to_top();
                        update_selected_symbol(&mut ui_state, &sim_engine, &strat_engine);
                    }
                    // Navigation: Bottom
                    KeyCode::Char('G') => {
                        ui_state.jump_to_bottom(max_rows);
                        update_selected_symbol(&mut ui_state, &sim_engine, &strat_engine);
                    }
                    // Navigation: Pane Switching (Watchlist -> Holdings -> Tradebook)
                    KeyCode::Tab => {
                        ui_state.cycle_pane();
                        update_selected_symbol(&mut ui_state, &sim_engine, &strat_engine);
                    }
                    // Interactive Chart View Toggle
                    KeyCode::Enter => {
                        ui_state.active_tab = match ui_state.active_tab {
                            ActiveTab::Chart => ActiveTab::Dashboard,
                            _ => ActiveTab::Chart,
                        };
                    }
                    KeyCode::Char('c') => {
                        ui_state.active_tab = match ui_state.active_tab {
                            ActiveTab::Chart => ActiveTab::Dashboard,
                            _ => ActiveTab::Chart,
                        };
                    }

                    // Navigation: Tabs
                    KeyCode::Char('h') | KeyCode::Left => {
                        ui_state.active_tab = match ui_state.active_tab {
                            ActiveTab::Dashboard => ActiveTab::Analytics,
                            ActiveTab::Chart => ActiveTab::Dashboard,
                            ActiveTab::Signals => ActiveTab::Chart,
                            ActiveTab::Analytics => ActiveTab::Signals,
                        };
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        ui_state.active_tab = match ui_state.active_tab {
                            ActiveTab::Dashboard => ActiveTab::Chart,
                            ActiveTab::Chart => ActiveTab::Signals,
                            ActiveTab::Signals => ActiveTab::Analytics,
                            ActiveTab::Analytics => ActiveTab::Dashboard,
                        };
                    }
                    KeyCode::Char('1') => ui_state.active_tab = ActiveTab::Dashboard,
                    KeyCode::Char('2') => ui_state.active_tab = ActiveTab::Chart,
                    KeyCode::Char('3') => ui_state.active_tab = ActiveTab::Signals,
                    KeyCode::Char('4') => ui_state.active_tab = ActiveTab::Analytics,

                    // Trading: Manual Order Entry (b: Buy, s: Sell)
                    KeyCode::Char('b') => {
                        let sym = ui_state.selected_symbol.clone()
                            .or_else(|| sim_engine.positions.keys().next().cloned())
                            .or_else(|| strat_engine.trackers.keys().next().cloned())
                            .unwrap_or_else(|| "HDFCBANK-EQ".to_string());
                        let ltp = strat_engine.trackers.get(&sym).map(|t| t.current_price).unwrap_or(100.0);
                        ui_state.order_modal.open(sym, PositionSide::Long, ltp);
                    }
                    KeyCode::Char('s') => {
                        let sym = ui_state.selected_symbol.clone()
                            .or_else(|| sim_engine.positions.keys().next().cloned())
                            .or_else(|| strat_engine.trackers.keys().next().cloned())
                            .unwrap_or_else(|| "HDFCBANK-EQ".to_string());
                        let ltp = strat_engine.trackers.get(&sym).map(|t| t.current_price).unwrap_or(100.0);
                        ui_state.order_modal.open(sym, PositionSide::Short, ltp);
                    }

                    // Trading: Algo Automation Toggle
                    KeyCode::Char('A') => {
                        sim_engine.algo_enabled = !sim_engine.algo_enabled;
                        let msg = if sim_engine.algo_enabled {
                            "🤖 ALGO EXECUTION: ENABLED (Autonomous Trading Active)"
                        } else {
                            "⏸ ALGO EXECUTION: PAUSED (Manual Trading Only)"
                        };
                        sim_engine.log_message(msg.to_string());
                        ui_state.set_status(msg);
                        let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                    }

                    // Trading: Square off selected holding
                    KeyCode::Char('x') => {
                        let mut sorted: Vec<String> = sim_engine.positions.keys().cloned().collect();
                        sorted.sort();
                        if let Some(target_sym) = sorted.get(ui_state.holdings_selected) {
                            if let Some(pos) = sim_engine.positions.remove(target_sym) {
                                let exit_price = pos.current_price;
                                sim_engine.close_trade(pos, exit_price, "ManualSquareOff");
                                ui_state.set_status(format!("Squared off {}", target_sym));
                                let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                            }
                        }
                    }

                    // Trading: PANIC - Close all open positions
                    KeyCode::Char('X') => {
                        let keys: Vec<String> = sim_engine.positions.keys().cloned().collect();
                        let count = keys.len();
                        for sym in keys {
                            if let Some(pos) = sim_engine.positions.remove(&sym) {
                                let exit_price = pos.current_price;
                                sim_engine.close_trade(pos, exit_price, "PanicExitAll");
                            }
                        }
                        ui_state.set_status(format!("PANIC: Squared off all {} positions", count));
                        let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
                    }

                    // Toggles
                    KeyCode::Char('d') => {
                        ui_state.show_debug = !ui_state.show_debug;
                    }
                    KeyCode::Char('?') | KeyCode::Char('/') => {
                        ui_state.show_help = !ui_state.show_help;
                    }

                    _ => {}
                }
            }
        }

        // C. Intraday Session Square-off Rule (15:15 IST)
        let now_local = chrono::Local::now();
        if now_local.hour() == 15 && now_local.minute() >= 15 && !sim_engine.positions.is_empty() {
            let keys: Vec<String> = sim_engine.positions.keys().cloned().collect();
            for sym in keys {
                if let Some(pos) = sim_engine.positions.remove(&sym) {
                    let exit_price = pos.current_price;
                    sim_engine.close_trade(pos, exit_price, "AutoSquareOff1515");
                }
            }
            let msg = "⏰ 15:15 IST INTRADAY AUTO-SQUARE OFF COMPLETE";
            sim_engine.log_message(msg.to_string());
            ui_state.set_status(msg);
            let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
        }

        // D. Ultra Low-Latency Ingest & Market Tick Evaluation
        tokio::select! {
            tick_opt = feed.next_tick() => {
                if let Some(tick) = tick_opt {
                    sim_engine.update_tick(&tick);
                    if let Some(signal) = strat_engine.on_tick(&tick) {
                        sim_engine.handle_signal(signal);
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(5)) => {}
        }

        // E. Periodic State Autosave (Every 5 seconds)
        if last_save.elapsed() >= Duration::from_secs(5) {
            let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
            last_save = Instant::now();
        }

        // F. Render Terminal Dashboard (Decoupled ~30 FPS)
        if last_render.elapsed() >= render_interval {
            terminal.draw(|f| render_ui(f, &sim_engine, &strat_engine, &ui_state))?;
            last_render = Instant::now();
        }
    }

    // 5. Restore Terminal State & Final State Persistence
    let _ = SessionStore::save(STATE_DIR, &sim_engine.to_session());
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show)?;
    terminal.show_cursor()?;

    println!("\n{}", "═══ Z-TRADE QUANT SESSION CLOSED ═══");
    println!("  Session Saved To:      {}/session_state.toml", STATE_DIR);
    println!("  Ticks Ingested:        {}", sim_engine.tick_count);
    println!("  Total Realized P&L:    ₹{:.2}", sim_engine.stats.realized_pnl);
    println!("  Total Trades Executed: {}", sim_engine.stats.trades_count);
    println!("  Win Rate:              {:.1}%\n", sim_engine.stats.win_rate());

    Ok(())
}

fn update_selected_symbol(
    ui_state: &mut UiState,
    sim_engine: &PaperSimulationEngine,
    strat_engine: &StrategyEngine,
) {
    match ui_state.active_pane {
        ActivePane::Watchlist => {
            let mut symbols: Vec<&String> = strat_engine.trackers.keys().collect();
            symbols.sort();
            if let Some(sym) = symbols.get(ui_state.watchlist_selected) {
                ui_state.selected_symbol = Some((*sym).clone());
            }
        }
        ActivePane::Holdings => {
            let mut positions: Vec<&String> = sim_engine.positions.keys().collect();
            positions.sort();
            if let Some(sym) = positions.get(ui_state.holdings_selected) {
                ui_state.selected_symbol = Some((*sym).clone());
            }
        }
        ActivePane::Tradebook => {
            if let Some(trade) = sim_engine.trades.iter().rev().nth(ui_state.tradebook_selected) {
                ui_state.selected_symbol = Some(trade.symbol.clone());
            }
        }
        ActivePane::Signals => {
            let signals: Vec<&trade_core::signal::DisplaySignal> = sim_engine.signal_feed.iter().rev().collect();
            if let Some(sig) = signals.get(ui_state.signals_selected) {
                ui_state.selected_symbol = Some(sig.symbol.clone());
            }
        }
        ActivePane::Chart => {}
    }
}
