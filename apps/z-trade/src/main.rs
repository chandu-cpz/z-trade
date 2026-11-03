use std::env;
use std::io::{self, stdout};
use std::time::{Duration, Instant};

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
use trade_feed::KotakFeed;
use trade_sim::PaperSimulationEngine;
use trade_strategy::StrategyEngine;
use trade_tui::{render_ui, ActivePane, ActiveTab, UiState};

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

    let config = Config::load_or_default("configs/default.toml");
    let mut sim_engine = PaperSimulationEngine::new(config.clone());
    let mut strat_engine = StrategyEngine::new(config.clone());
    let mut ui_state = UiState::new();

    // 1. Authenticate with Kotak Neo API
    let access_token = env::var("KOTAK_ACCESS_TOKEN").expect("KOTAK_ACCESS_TOKEN missing in .env");
    let mobile_number = env::var("KOTAK_MOBILE_NUMBER").expect("KOTAK_MOBILE_NUMBER missing in .env");
    let ucc = env::var("KOTAK_UCC").expect("KOTAK_UCC missing in .env");
    let totp_secret = env::var("KOTAK_TOTP_SECRET").expect("KOTAK_TOTP_SECRET missing in .env");
    let mpin = env::var("KOTAK_MPIN").expect("KOTAK_MPIN missing in .env");

    let mut api = KotakNeo::new();
    api.login(&access_token, &mobile_number, &ucc, &totp_secret, &mpin).await?;
    sim_engine.log_message("Authenticated with Kotak Neo API.".to_string());
    ui_state.set_status("Connected to Kotak Neo");

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

    // 4. Main Event Loop
    loop {
        // A. Handle Vim & Keyboard Controls
        if event::poll(Duration::from_millis(1))? {
            if let Event::Key(key) = event::read()? {
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
                    ActivePane::Holdings => sim_engine.positions.len(),
                    ActivePane::Tradebook => sim_engine.trades.len(),
                    ActivePane::Signals => sim_engine.signal_feed.len(),
                };

                match key.code {
                    // Navigation: Next Row
                    KeyCode::Char('j') | KeyCode::Down => {
                        ui_state.next_row(max_rows);
                    }
                    // Navigation: Prev Row
                    KeyCode::Char('k') | KeyCode::Up => {
                        ui_state.prev_row();
                    }
                    // Navigation: Top
                    KeyCode::Char('g') => {
                        ui_state.jump_to_top();
                    }
                    // Navigation: Bottom
                    KeyCode::Char('G') => {
                        ui_state.jump_to_bottom(max_rows);
                    }
                    // Navigation: Pane Switching
                    KeyCode::Tab => {
                        ui_state.cycle_pane();
                    }
                    // Navigation: Tabs
                    KeyCode::Char('h') | KeyCode::Left => {
                        ui_state.active_tab = match ui_state.active_tab {
                            ActiveTab::Dashboard => ActiveTab::Analytics,
                            ActiveTab::Signals => ActiveTab::Dashboard,
                            ActiveTab::Analytics => ActiveTab::Signals,
                        };
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        ui_state.active_tab = match ui_state.active_tab {
                            ActiveTab::Dashboard => ActiveTab::Signals,
                            ActiveTab::Signals => ActiveTab::Analytics,
                            ActiveTab::Analytics => ActiveTab::Dashboard,
                        };
                    }
                    KeyCode::Char('1') => ui_state.active_tab = ActiveTab::Dashboard,
                    KeyCode::Char('2') => ui_state.active_tab = ActiveTab::Signals,
                    KeyCode::Char('3') => ui_state.active_tab = ActiveTab::Analytics,

                    // Trading: Square off selected holding
                    KeyCode::Char('x') => {
                        let mut sorted: Vec<String> = sim_engine.positions.keys().cloned().collect();
                        sorted.sort();
                        if let Some(target_sym) = sorted.get(ui_state.holdings_selected) {
                            if let Some(pos) = sim_engine.positions.remove(target_sym) {
                                let exit_price = pos.current_price;
                                sim_engine.close_trade(pos, exit_price, "ManualSquareOff");
                                ui_state.set_status(format!("Squared off {}", target_sym));
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
                    }

                    // Toggles
                    KeyCode::Char('d') => {
                        ui_state.show_debug = !ui_state.show_debug;
                    }
                    KeyCode::Char('?') | KeyCode::Char('/') => {
                        ui_state.show_help = !ui_state.show_help;
                    }

                    // Exit
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }

        // B. Ingest and Process Market Ticks
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

        // C. Render Terminal UI
        if last_render.elapsed() >= render_interval {
            terminal.draw(|f| render_ui(f, &sim_engine, &ui_state))?;
            last_render = Instant::now();
        }
    }

    // 5. Restore Terminal State
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show)?;
    terminal.show_cursor()?;

    println!("\n{}", "═══ Z-TRADE SESSION CLOSED ═══");
    println!("  Ticks Processed:       {}", sim_engine.tick_count);
    println!("  Total Realized P&L:    ₹{:.2}", sim_engine.stats.realized_pnl);
    println!("  Total Trades Executed: {}", sim_engine.stats.trades_count);
    println!("  Win Rate:              {:.1}%\n", sim_engine.stats.win_rate());

    Ok(())
}
