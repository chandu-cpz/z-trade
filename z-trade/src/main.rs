use std::env;
use std::hash::{Hash, Hasher};
use std::io;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

use chrono::Local;
use crossterm::{
    cursor::Show,
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use dotenvy::dotenv;
use kotak_neo_api::{ExchangeSegment, FeedType, KotakNeo, MarketEvent, Scrip, ScripListReader};
use ratatui::{backend::CrosstermBackend, Terminal};

mod config;
mod indicators;
mod models;
mod sim;
mod strategy;
mod ui;
mod worker;

use config::Config;
use sim::PaperEngine;
use ui::render_ui;
use worker::{SignalMsg, Worker, WorkerMsg};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();

    let config = Config::default();

    // Terminal crash safety panic hook
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        let _ = io::stdout().execute(Show);
        default_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut engine = PaperEngine::new(config.clone());

    // Login to Kotak Neo
    let access_token = env::var("KOTAK_ACCESS_TOKEN").expect("KOTAK_ACCESS_TOKEN missing");
    let mobile_number = env::var("KOTAK_MOBILE_NUMBER").expect("KOTAK_MOBILE_NUMBER missing");
    let ucc = env::var("KOTAK_UCC").expect("KOTAK_UCC missing");
    let totp_secret = env::var("KOTAK_TOTP_SECRET").expect("KOTAK_TOTP_SECRET missing");
    let mpin = env::var("KOTAK_MPIN").expect("KOTAK_MPIN missing");

    let mut api = KotakNeo::new();
    api.login(&access_token, &mobile_number, &ucc, &totp_secret, &mpin).await?;

    // Load equity universe
    let stocks = ScripListReader::get_equity_stocks(ExchangeSegment::NseCm)?;
    let scrips: Vec<Scrip> = stocks
        .iter()
        .filter(|s| s.is_equity())
        .take(2413)
        .map(|s| Scrip::new(ExchangeSegment::NseCm, &s.token))
        .collect();

    engine.add_message(format!("Subscribing to {} stocks...", scrips.len()));

    let mut market = api.market_feed().await?;
    let _orders = api.order_feed().await?;
    market.subscribe(&scrips, FeedType::Scrip).await?;
    engine.add_message("Connected to market feed. Spawning parallel workers...".to_string());

    // Spawn Parallel Workers
    let (signal_tx, signal_rx) = channel::<SignalMsg>();
    let num_workers = config.num_workers;

    for i in 0..num_workers {
        let (worker_tx, worker_rx) = channel::<WorkerMsg>();
        engine.workers.push(worker_tx);
        let s_tx = signal_tx.clone();
        let cfg = config.clone();

        thread::spawn(move || {
            let worker = Worker::new(i, s_tx, cfg);
            worker.run(worker_rx);
        });
    }

    let mut last_render = std::time::Instant::now();
    let render_interval = Duration::from_millis(33);
    let mut disconnected = false;

    // Main event loop
    loop {
        // 1. Ingest Market Feed Ticks
        loop {
            match market.rx.try_recv() {
                Ok(ev) => match ev {
                    MarketEvent::Connected => {
                        engine.add_message("Market feed connected".to_string());
                    }
                    MarketEvent::Tick(tick) => {
                        engine.tick_count += 1;
                        let now_inst = std::time::Instant::now();
                        engine.tick_times.push_back(now_inst);
                        while let Some(front) = engine.tick_times.front() {
                            if now_inst.duration_since(*front) > Duration::from_secs(3) {
                                engine.tick_times.pop_front();
                            } else {
                                break;
                            }
                        }

                        let sym = tick.trading_symbol.as_deref().unwrap_or(&tick.token).to_string();

                        if engine.debug_mode {
                            let ltp_str = tick.data.get("ltp").cloned().unwrap_or_default();
                            let vol = tick.data.get("v").cloned().unwrap_or_default();
                            engine.debug_ticks.push_back(format!(
                                "{} {} {} vo:{}",
                                Local::now().format("%H:%M:%S%.3f"),
                                sym,
                                ltp_str,
                                vol
                            ));
                            if engine.debug_ticks.len() > 500 {
                                engine.debug_ticks.pop_front();
                            }
                        }

                        if let Some(ltp_str) = tick.data.get("ltp") {
                            if let Ok(current_ltp) = ltp_str.parse::<f64>() {
                                // Check paper position exits
                                let exit_info = if let Some(position) = engine.positions.get_mut(&sym) {
                                    position.update_price(current_ltp);
                                    let (exit, reason) = position.should_exit(&engine.config);
                                    if exit {
                                        Some(reason)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };

                                if let Some(reason) = exit_info {
                                    if let Some(pos) = engine.positions.remove(&sym) {
                                        engine.close_trade(pos, current_ltp, &reason);
                                    }
                                }

                                // Shard tick across parallel worker threads
                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                sym.hash(&mut hasher);
                                let worker_idx = (hasher.finish() as usize) % num_workers;

                                let volume = tick
                                    .data
                                    .get("v")
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .unwrap_or(0);
                                let tbq = tick
                                    .data
                                    .get("tbq")
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .unwrap_or(0);
                                let tsq = tick
                                    .data
                                    .get("tsq")
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .unwrap_or(0);
                                let bp = tick
                                    .data
                                    .get("bp")
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .unwrap_or(0.0);
                                let sp = tick
                                    .data
                                    .get("sp")
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .unwrap_or(0.0);
                                let ltq = tick
                                    .data
                                    .get("ltq")
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .unwrap_or(0);

                                let _ = engine.workers[worker_idx].send(WorkerMsg::Tick {
                                    token: sym,
                                    price: current_ltp,
                                    volume,
                                    total_buy_qty: tbq,
                                    total_sell_qty: tsq,
                                    best_bid: bp,
                                    best_ask: sp,
                                    last_trade_qty: ltq,
                                    timestamp: Local::now(),
                                });
                            }
                        }
                    }
                    MarketEvent::Disconnected(_) => {
                        disconnected = true;
                    }
                },
                Err(_) => break,
            }
        }

        // 2. Process Worker Signals into Paper Simulation
        loop {
            match signal_rx.try_recv() {
                Ok(msg) => {
                    engine.handle_signal(msg);
                }
                Err(_) => break,
            }
        }

        // 3. Render Dashboard UI
        if last_render.elapsed() >= render_interval {
            terminal.draw(|f| render_ui(f, &engine))?;
            last_render = std::time::Instant::now();

            if event::poll(Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => {
                            disconnected = true;
                        }
                        KeyCode::Char('d') => {
                            engine.debug_mode = !engine.debug_mode;
                        }
                        _ => {}
                    }
                }
            }
        }

        if disconnected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    // Terminal cleanup
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;

    println!("\n═══ FINAL PAPER TRADING SUMMARY ═══");
    println!("Total P&L: ₹{:.2}", engine.total_portfolio_pnl());
    println!(
        "Trades: {} ({}W/{}L)",
        engine.stats.trades_count, engine.stats.winning_trades, engine.stats.losing_trades
    );
    println!("Win Rate: {:.1}%", engine.stats.win_rate());

    println!(
        "\n┌Trade Log ({})─────────────────────────────────────────────────────────────────┐",
        engine.trades.len()
    );
    println!("│Time     Symbol    Side P&L      Reason                                         │");
    for trade in engine.trades.iter() {
        let side_str = format!("{}", trade.side);
        let time_str = trade.exit_time.format("%H:%M:%S");
        println!(
            "│{} {:<9} {:<5} {:<8.2} {:<45} │",
            time_str, trade.symbol, side_str, trade.pnl, trade.exit_reason
        );
    }
    println!("└────────────────────────────────────────────────────────────────────────────────┘");

    Ok(())
}
