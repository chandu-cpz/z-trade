use dotenvy::dotenv;
use std::env;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::Duration;
use std::sync::mpsc::{channel, Sender};
use std::thread;

use kotak_neo_api::{KotakNeo, Scrip, ExchangeSegment, FeedType, MarketEvent, ScripListReader};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Cell},
    Frame, Terminal,
};
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    cursor::Show,
    ExecutableCommand,
};
use chrono::{DateTime, Local};

mod models;
mod worker;
use models::*;
use worker::*;

// Redefining AppState as App for clarity and removing unused structs
pub struct App {
    pub positions: HashMap<String, Position>,
    pub signal_feed: VecDeque<DisplaySignal>,
    pub trades: Vec<Trade>,
    pub stats: TradingStats,
    pub locked_capital: f64,
    pub initial_capital: f64,
    pub circuit_breaker_hit: bool,
    pub debug_mode: bool,
    pub messages: VecDeque<String>,
    pub tick_count: u64,
    pub tick_times: VecDeque<std::time::Instant>,
    pub _timestamp: DateTime<Local>,
    pub last_update: DateTime<Local>,
    pub debug_ticks: VecDeque<String>,
    pub workers: Vec<Sender<WorkerMsg>>,
}

impl App {
    fn new() -> Self {
        Self {
            positions: HashMap::new(),
            signal_feed: VecDeque::with_capacity(50),
            trades: Vec::new(),
            stats: TradingStats::new(),
            locked_capital: 0.0,
            initial_capital: 20000.0,
            circuit_breaker_hit: false,
            debug_mode: false,
            messages: VecDeque::with_capacity(10),
            tick_count: 0,
            tick_times: VecDeque::new(),
            _timestamp: Local::now(),
            last_update: Local::now(),
            debug_ticks: VecDeque::with_capacity(500),
            workers: Vec::new(),
        }
    }

    fn add_message(&mut self, msg: String) {
        let time_str = Local::now().format("%H:%M:%S");
        self.messages.push_back(format!("{} {}", time_str, msg));
        if self.messages.len() > 10 {
            self.messages.pop_front();
        }
    }

    fn add_signal(&mut self, symbol: &str, side: PositionSide, strategy: Strategy, entered: bool, reason: Option<&str>) {
        let sig = DisplaySignal {
            time: Local::now(),
            symbol: symbol.to_string(),
            side,
            strategy,
            entered,
            block_reason: reason.map(|s| s.to_string()),
        };
        self.signal_feed.push_back(sig);
        if self.signal_feed.len() > 30 {
            self.signal_feed.pop_front();
        }
    }

    fn free_capital(&self) -> f64 {
        self.initial_capital + self.stats.realized_pnl - self.locked_capital
    }

    fn total_value(&self) -> f64 {
        self.free_capital() + self.locked_capital + self.unrealized_pnl()
    }

    fn unrealized_pnl(&self) -> f64 {
        self.positions.values().map(|p| p.unrealized_pnl()).sum()
    }

    fn total_portfolio_pnl(&self) -> f64 {
        self.stats.realized_pnl + self.unrealized_pnl()
    }


    fn get_max_positions(&self) -> usize { SCALP_MAX_POSITIONS }
    fn get_max_loss(&self) -> f64 { SCALP_MAX_LOSS }

    fn can_take_trade(&self) -> bool {
        self.positions.len() < SCALP_MAX_POSITIONS
        && !self.circuit_breaker_hit
        && self.free_capital() > 100.0
    }

    fn close_trade(&mut self, position: Position, exit_price: f64, reason: &str) {
        let pnl = match position.side {
            PositionSide::Long => (exit_price - position.entry_price) * position.quantity as f64,
            PositionSide::Short => (position.entry_price - exit_price) * position.quantity as f64,
        };
        
        // Release capital
        let locked = position.entry_price * position.quantity as f64;
        self.locked_capital = (self.locked_capital - locked).max(0.0);

        // Update Stats
        self.stats.record_trade(pnl);

        let now = Local::now();

        // Check Circuit Breaker
        if self.total_portfolio_pnl() <= -self.get_max_loss() {
            self.circuit_breaker_hit = true;
            self.add_message(format!("⚠ CIRCUIT BREAKER HIT at ₹{:.2} loss!", -self.total_portfolio_pnl()));
        }

        // Log
        self.add_message(format!("EXIT {} @ {:.2} | P&L: ₹{:.2} | {}", position.symbol, exit_price, pnl, reason));

        // Record Trade
        self.trades.push(Trade {
            symbol: position.symbol.clone(),
            side: position.side,
            strategy: position.strategy,
            entry_price: position.entry_price,
            exit_price,
            quantity: position.quantity,
            pnl,
            entry_time: position.entry_time,
            exit_time: now,
            exit_reason: reason.to_string(),
        });
    }

    fn calculate_position_size(&self, price: f64, uas: f64) -> u32 {
        let mut max_value = self.free_capital().min(5000.0); // Dynamic cap (Base)

        // Signal-Weighted Sizing (SWS)
        if uas > 90.0 {
            max_value *= 1.5; // High conviction boost
        } else if uas > 80.0 {
            max_value *= 1.25;
        }

        if price <= 0.0 { return 1; }
        
        if price > SCALP_MAX_PRICE {
            return 0;
        }

        let shares = (max_value / price) as u32;
        let (min_s, max_s) = (1u32, 100u32); 
        shares.max(min_s).min(max_s)
    }

    fn handle_signal(&mut self, msg: SignalMsg) {
        // Gate 3: Loss Cooldown Check
        // We need to know previous loss time?
        // App stores Trades. We can query Trades for last loss on this symbol.
        // O(N) scan on trades?
        // Optimization: `last_loss_map: HashMap<String, DateTime<Local>>` in App?
        // For now, scan trades (reversed) is cheap enough if trades list isn't huge (1000 items is fast).
        
        let sym = &msg.symbol;
        let now_time = Local::now();

        let cooldown_ok = {
            let last_loss = self.trades.iter().rev()
                .find(|t| t.symbol == *sym && t.pnl < 0.0)
                .map(|t| t.exit_time);
            
            if let Some(t) = last_loss {
                 now_time.signed_duration_since(t).num_seconds() >= LOSS_COOLDOWN_SECS
            } else {
                true
            }
        };

        if !cooldown_ok {
            self.add_signal(sym, msg.side, msg.strategy, false, Some("loss cooldown"));
            return;
        }

        if self.positions.contains_key(sym) {
             return;
        }

        // --- Phase 12: Velocity Scalper Rotation ---
        
        // 1. Dynamic Hurdle Logic (In-Memory Only)
        let utilization = self.positions.len() as f64 / SCALP_MAX_POSITIONS as f64;
        let dynamic_threshold = IES_BASE_THRESHOLD + (utilization * (IES_MAX_THRESHOLD - IES_BASE_THRESHOLD));
        
        if msg.uas < dynamic_threshold {
             self.add_signal(sym, msg.side, msg.strategy, false, Some(&format!("IES < {:.0}", dynamic_threshold)));
             return;
        }

        let mut proceed = false;
        let can_afford = self.free_capital() >= (msg.price * 1.0);
        
        if self.can_take_trade() && can_afford {
            proceed = true;
        } else {
            // AGGRESSIVE ROTATION: Find the "deadest" trade by alpha_score
            let victim = self.positions.iter()
                .map(|(k, p)| (k, p.alpha_score()))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((lazy_sym, old_score)) = victim {
                // If new impulse is 20%+ stronger than existing weakest, SWAP IMMEDIATELY
                if msg.uas > (old_score + IES_SWAP_ADVANTAGE) {
                    let lazy_sym_str = lazy_sym.to_string();
                    if let Some(pos_removed) = self.positions.remove(&lazy_sym_str) {
                         let exit_price = pos_removed.current_price;
                         self.close_trade(pos_removed, exit_price, "VelocitySwap");
                         self.add_message(format!("SWAP: Rotating {} (IES:{:.0}) -> {} (IES:{:.0})", lazy_sym_str, old_score, sym, msg.uas));
                         proceed = true;
                    }
                }
            }
        }

        if proceed {
            let qty = self.calculate_position_size(msg.price, msg.uas);
            if qty > 0 {
                let position = Position::new(sym.clone(), msg.side, msg.strategy, msg.price, qty, TradingMode::ScalpMom);
                self.locked_capital += msg.price * qty as f64;
                self.add_message(format!("ENTER {} {} {} @{:.2} x{}",
                    sym, msg.side, msg.strategy, msg.price, qty));
                self.add_signal(sym, msg.side, msg.strategy, true, None);
                self.positions.insert(sym.clone(), position);
            } else {
                self.add_signal(sym, msg.side, msg.strategy, false, Some("price > limit"));
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// UI RENDERING
// ═══════════════════════════════════════════════════════════════════════════

fn render_ui(frame: &mut Frame, app: &App) {
    let size = frame.area();

    if app.debug_mode {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(0)])
            .split(size);
        render_portfolio_summary(frame, app, main_chunks[0]);
        render_debug_ticks(frame, app, main_chunks[1]);
    } else {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(0)])
            .split(size);

        render_portfolio_summary(frame, app, main_chunks[0]);

        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(main_chunks[1]);

        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(content_chunks[0]);

        render_holdings(frame, app, left_chunks[0]);
        render_tradebook(frame, app, left_chunks[1]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(content_chunks[1]);

        render_signal_feed(frame, app, right_chunks[0]);
        render_strategy_stats(frame, app, right_chunks[1]);
    }
}

fn render_signal_feed(frame: &mut Frame, app: &App, area: Rect) {
    let visible = area.height.saturating_sub(3) as usize;
    let header = Line::from(vec![
        Span::styled(format!("{:<8}", "Time"),    Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<10}", "Symbol"),  Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<5}", "Side"),     Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<9}", "Strategy"), Style::default().fg(Color::DarkGray)),
        Span::styled("Status".to_string(),         Style::default().fg(Color::DarkGray)),
    ]);

    let mut lines: Vec<Line> = vec![header];
    for sig in app.signal_feed.iter().rev().take(visible) {
        let time_str = sig.time.format("%H:%M:%S").to_string();
        let sym = if sig.symbol.len() > 9 { &sig.symbol[..9] } else { &sig.symbol };
        let side_color = if sig.side == PositionSide::Long { Color::Green } else { Color::Red };
        let side_str = if sig.side == PositionSide::Long { "LONG " } else { "SHORT" };
        let (status_str, status_color) = if sig.entered {
            ("✓ ENTERED".to_string(), Color::Green)
        } else {
            let reason = sig.block_reason.as_deref().unwrap_or("blocked");
            (format!("✗ {}", reason), Color::Yellow)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<8}", time_str), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<10}", sym),      Style::default().fg(Color::White)),
            Span::styled(format!("{:<5}", side_str),  Style::default().fg(side_color)),
            Span::styled(format!("{:<9}", sig.strategy.to_string()), Style::default().fg(Color::Cyan)),
            Span::styled(status_str,                  Style::default().fg(status_color)),
        ]));
    }

    let block = Block::default()
        .title(format!(" Signal Feed ({} total) ", app.signal_feed.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_strategy_stats(frame: &mut Frame, app: &App, area: Rect) {
    let mut stats: HashMap<String, (u32, u32, f64)> = HashMap::new();
    for trade in &app.trades {
        let key = trade.strategy.to_string();
        let entry = stats.entry(key).or_insert((0, 0, 0.0));
        entry.0 += 1; 
        if trade.pnl > 0.0 { entry.1 += 1; }
        entry.2 += trade.pnl;
    }

    let header = Line::from(vec![
        Span::styled(format!("{:<9}", "Strategy"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>6}", "Trades"),   Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>7}", "WR%"),       Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>9}", "Total P&L"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>8}", "Avg P&L"),   Style::default().fg(Color::DarkGray)),
    ]);

    let mut lines: Vec<Line> = vec![header];
    let mut sorted: Vec<_> = stats.iter().collect();
    sorted.sort_by(|a, b| b.1.2.partial_cmp(&a.1.2).unwrap_or(std::cmp::Ordering::Equal));

    for (strat, (total, wins, total_pnl)) in &sorted {
        let wr = if *total > 0 { *wins as f64 / *total as f64 * 100.0 } else { 0.0 };
        let avg_pnl = if *total > 0 { total_pnl / *total as f64 } else { 0.0 };
        let pnl_color = if *total_pnl >= 0.0 { Color::Green } else { Color::Red };
        let wr_color = if wr >= 50.0 { Color::Green } else if wr >= 40.0 { Color::Yellow } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<9}", strat),              Style::default().fg(Color::Cyan)),
            Span::styled(format!("{:>6}", total),              Style::default().fg(Color::White)),
            Span::styled(format!("{:>6.1}%", wr),             Style::default().fg(wr_color)),
            Span::styled(format!("{:>9.2}", total_pnl),        Style::default().fg(pnl_color)),
            Span::styled(format!("{:>8.2}", avg_pnl),          Style::default().fg(pnl_color)),
        ]));
    }

    let block = Block::default().title(" Strategy Stats ").borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_debug_ticks(frame: &mut Frame, app: &App, area: Rect) {
    let visible = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = app.debug_ticks.iter()
        .rev()
        .take(visible)
        .rev()
        .map(|s| {
            let color = if s.contains('▲') { Color::Green } else if s.contains('▼') { Color::Red } else { Color::DarkGray };
            Line::from(Span::styled(s.clone(), Style::default().fg(color)))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("🔍 LIVE TICKS [d] to close | {} ticks buffered", app.debug_ticks.len()))
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_portfolio_summary(frame: &mut Frame, app: &App, area: Rect) {
    let total_pnl = app.total_portfolio_pnl();
    let unrealized = app.unrealized_pnl();
    let realized = app.stats.realized_pnl;
    let free = app.free_capital();
    let locked = app.locked_capital;
    let total_val = app.total_value();
    let win_rate = app.stats.win_rate();
    let max_pos = app.get_max_positions();
    let max_loss = app.get_max_loss();

    let pnl_color = if total_pnl >= 0.0 { Color::Green } else { Color::Red };
    
    let status = if app.circuit_breaker_hit {
        Span::styled(" ⚠ CIRCUIT BREAKER ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" ✓ ACTIVE ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    };

    let mode_display = Span::styled(" [3] SCALP (Momentum) ",   Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let ticks_per_sec = {
        let now_inst = std::time::Instant::now();
        let window = std::time::Duration::from_secs(1);
        let count = app.tick_times.iter().rev()
            .take_while(|t| now_inst.duration_since(**t) < window)
            .count();
        count as f64
    };

    let text = vec![
        Line::from(vec![
            Span::styled("═══ PORTFOLIO ═══", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            status,
            Span::raw("  "),
            mode_display,
        ]),
        Line::from(vec![
            Span::raw("Total P&L: "),
            Span::styled(format!("₹{:.2}", total_pnl), Style::default().fg(pnl_color).add_modifier(Modifier::BOLD)),
            Span::raw("  |  Realized: "),
            Span::styled(format!("₹{:.2}", realized), Style::default().fg(if realized >= 0.0 { Color::Green } else { Color::Red })),
            Span::raw("  |  Unrealized: "),
            Span::styled(format!("₹{:.2}", unrealized), Style::default().fg(if unrealized >= 0.0 { Color::Green } else { Color::Yellow })),
        ]),
        Line::from(vec![
            Span::raw("Free: "),
            Span::styled(format!("₹{:.0}", free), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("  |  Locked: "),
            Span::styled(format!("₹{:.0}", locked), Style::default().fg(Color::Yellow)),
            Span::raw("  |  Portfolio: "),
            Span::styled(format!("₹{:.0}", total_val), Style::default().fg(Color::Cyan)),
            Span::raw("  |  Trades: "),
            Span::styled(format!("{}", app.stats.trades_count), Style::default()),
            Span::raw("  |  Win: "),
            Span::styled(format!("{:.1}%", win_rate), Style::default().fg(if win_rate >= 50.0 { Color::Green } else { Color::Red })),
        ]),
        Line::from(vec![
            Span::raw("Positions: "),
            Span::styled(format!("{}/{}", app.positions.len(), max_pos), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw("  |  "),
            Span::raw(format!("MaxLoss: ₹{:.0}", max_loss)),
            Span::raw("  |  Ticks: "),
            Span::styled(format!("{}", app.tick_count), Style::default().fg(Color::Green)),
            Span::raw(" ("),
            Span::styled(format!("{:.0}/s", ticks_per_sec), Style::default().fg(Color::Yellow)),
            Span::raw(")  "),
            if app.debug_mode {
                Span::styled("[d] DEBUG ON", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("[d] debug", Style::default().fg(Color::DarkGray))
            },
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Portfolio Summary"));
    frame.render_widget(paragraph, area);
}

fn render_holdings(frame: &mut Frame, app: &App, area: Rect) {
    let mut rows = vec![];
    let mut positions: Vec<(&String, &Position)> = app.positions.iter().collect();
    positions.sort_by(|a, b| b.1.unrealized_pnl().partial_cmp(&a.1.unrealized_pnl()).unwrap_or(std::cmp::Ordering::Equal));

    for (sym, pos) in &positions {
        let pnl = pos.unrealized_pnl();
        let pnl_pct = pos.pnl_percent();
        let locked = pos.entry_price * pos.quantity as f64;
        
        let pnl_color = if pnl >= 0.0 { Color::Green } else { Color::Red };
        let side_color = if matches!(pos.side, PositionSide::Long) { Color::Cyan } else { Color::Yellow };
        let sym_display = if sym.len() > 6 { &sym[..6] } else { sym.as_str() };
        
        rows.push(Row::new(vec![
            Cell::from(sym_display.to_string()),
            Cell::from(format!("{}", pos.side)).style(Style::default().fg(side_color)),
            Cell::from(format!("{}", pos.quantity)),
            Cell::from(format!("{:.2}", pos.entry_price)),
            Cell::from(format!("{:.2}", pos.current_price)),
            Cell::from(format!("{:.0}", locked)).style(Style::default().fg(Color::DarkGray)),
            Cell::from(format!("{:.2}", pnl)).style(Style::default().fg(pnl_color).add_modifier(Modifier::BOLD)),
            Cell::from(format!("{:.2}%", pnl_pct)).style(Style::default().fg(pnl_color)),
        ]));
    }

    let table = Table::new(
        rows,
        &[
            Constraint::Percentage(14), Constraint::Percentage(9), Constraint::Percentage(7),
            Constraint::Percentage(13), Constraint::Percentage(13), Constraint::Percentage(12),
            Constraint::Percentage(16), Constraint::Percentage(16),
        ]
    ).header(
        Row::new(vec!["Symb", "Side", "Qty", "Entry", "LTP", "Locked", "P&L", "P&L%"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    ).block(Block::default().borders(Borders::ALL).title(format!("Holdings ({}/{})", app.positions.len(), app.get_max_positions())));

    frame.render_widget(table, area);
}

fn render_tradebook(frame: &mut Frame, app: &App, area: Rect) {
    let mut rows = vec![];
    for trade in app.trades.iter().rev().take(20) {
        let pnl_color = if trade.pnl >= 0.0 { Color::Green } else { Color::Red };
        rows.push(Row::new(vec![
            Cell::from(trade.exit_time.format("%H:%M:%S").to_string()),
            Cell::from(trade.symbol.clone()),
            Cell::from(format!("{}", trade.side)),
            Cell::from(format!("{}x{}", trade.entry_price, trade.quantity)),
            Cell::from(format!("{:.2}", trade.exit_price)),
            Cell::from(format!("{:.2}", trade.pnl)).style(Style::default().fg(pnl_color).add_modifier(Modifier::BOLD)),
            Cell::from(trade.exit_reason.clone()),
        ]));
    }

    let table = Table::new(
        rows,
        &[
            Constraint::Percentage(12), Constraint::Percentage(18), Constraint::Percentage(10),
            Constraint::Percentage(15), Constraint::Percentage(12), Constraint::Percentage(15), Constraint::Percentage(18),
        ]
    ).header(
        Row::new(vec!["Time", "Symbol", "Side", "Entry", "Exit", "P&L", "Reason"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    ).block(Block::default().borders(Borders::ALL).title(format!("Trade Book (Last 20 of {})", app.trades.len())));

    frame.render_widget(table, area);
}

// ═══════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();

    // Panic hook
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        let _ = io::stdout().execute(Show);
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    // Login
    let access_token = env::var("KOTAK_ACCESS_TOKEN").expect("KOTAK_ACCESS_TOKEN missing");
    let mobile_number = env::var("KOTAK_MOBILE_NUMBER").expect("KOTAK_MOBILE_NUMBER missing");
    let ucc = env::var("KOTAK_UCC").expect("KOTAK_UCC missing");
    let totp_secret = env::var("KOTAK_TOTP_SECRET").expect("KOTAK_TOTP_SECRET missing");
    let mpin = env::var("KOTAK_MPIN").expect("KOTAK_MPIN missing");

    let mut api = KotakNeo::new();
    api.login(&access_token, &mobile_number, &ucc, &totp_secret, &mpin).await?;

    let stocks = ScripListReader::get_equity_stocks(ExchangeSegment::NseCm)?;
    let scrips: Vec<Scrip> = stocks.iter()
        .filter(|s| s.is_equity())
        .take(2413) // Full subscription
        .map(|s| Scrip::new(ExchangeSegment::NseCm, &s.token))
        .collect();

    app.add_message(format!("Subscribing to {} stocks...", scrips.len()));

    let mut market = api.market_feed().await?;
    let _orders = api.order_feed().await?;
    market.subscribe(&scrips, FeedType::Scrip).await?;
    app.add_message("Connected to market data. Starting parallel workers...".to_string());

    // ═══════════════════════════════════════════════════════════════════════
    // PARALLEL WORKER SETUP
    // ═══════════════════════════════════════════════════════════════════════
    let (signal_tx, signal_rx) = channel::<SignalMsg>();
    let num_workers = 8; // Parallelize across 8 logical cores
    
    for i in 0..num_workers {
        let (worker_tx, worker_rx) = channel::<WorkerMsg>();
        app.workers.push(worker_tx);
        let s_tx = signal_tx.clone();
        
        thread::spawn(move || {
            let worker = Worker::new(i, s_tx);
            worker.run(worker_rx);
        });
    }

    let mut last_render = std::time::Instant::now();
    let render_interval = Duration::from_millis(33);
    let mut disconnected = false;

    // MAIN LOOP
    loop {
        let mut _drained = 0u64;

        // 1. Drain Market -> Dispatch to Workers
        loop {
            match market.rx.try_recv() {
                Ok(ev) => {
                    match ev {
                        MarketEvent::Connected => { app.add_message("Market connected".to_string()); }
                        MarketEvent::Tick(tick) => {
                             app.tick_count += 1;
                             let now_inst = std::time::Instant::now();
                             app.tick_times.push_back(now_inst);
                             while let Some(front) = app.tick_times.front() {
                                if now_inst.duration_since(*front) > Duration::from_secs(3) {
                                    app.tick_times.pop_front();
                                } else { break; }
                             }

                             let sym = tick.trading_symbol.as_deref().unwrap_or(&tick.token).to_string();
                             
                             // Debug logging
                             if app.debug_mode {
                                let ltp_str = tick.data.get("ltp").cloned().unwrap_or_default();
                                let vol = tick.data.get("v").cloned().unwrap_or_default();
                                app.debug_ticks.push_back(format!("{} {} {} vo:{}", 
                                    Local::now().format("%H:%M:%S%.3f"), sym, ltp_str, vol));
                                if app.debug_ticks.len() > 500 { app.debug_ticks.pop_front(); }
                             }

                             // Update Position (P&L tracking in Main Thread)
                             if let Some(ltp_str) = tick.data.get("ltp") {
                                 if let Ok(current_ltp) = ltp_str.parse::<f64>() {
                                      // Check exits
                                      if let Some(position) = app.positions.get_mut(&sym) {
                                          position.update_price(current_ltp);
                                          let (should_exit, _reason) = position.should_exit(TradingMode::ScalpMom);
                                          if should_exit {
                                              // We need to close it. But `position` is borrowed mutably.
                                              // We need to defer close?
                                              // Or clone reason and close after borrow ends?
                                              // HashMap entry api holds borrow.
                                              // Workaround: We remove it if exit needed.
                                              // But `get_mut` returns `&mut V`. We can't remove while holding `&mut V`.
                                              // We just returned `should_exit`.
                                          }
                                      }
                                      // Actually, safely closing in Rust requires two steps or entry API.
                                      // Let's do:
                                      let exit_info = if let Some(position) = app.positions.get_mut(&sym) {
                                          position.update_price(current_ltp);
                                          let (exit, reason) = position.should_exit(TradingMode::ScalpMom);
                                          if exit { Some(reason) } else { None }
                                      } else { None };
                                      
                                      if let Some(reason) = exit_info {
                                          if let Some(pos) = app.positions.remove(&sym) {
                                              app.close_trade(pos, current_ltp, &reason);
                                          }
                                      }

                                      // Dispatch to Worker
                                      // Simple sharding by hash of token
                                      let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                      use std::hash::{Hash, Hasher};
                                      sym.hash(&mut hasher);
                                      let worker_idx = (hasher.finish() as usize) % num_workers;
                                      
                                      // Parse fields for WorkerMsg
                                      let volume = tick.data.get("v").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                                      let tbq = tick.data.get("tbq").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                                      let tsq = tick.data.get("tsq").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                                      let bp = tick.data.get("bp").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                                      let sp = tick.data.get("sp").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                                      let ltq = tick.data.get("ltq").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

                                      let _ = app.workers[worker_idx].send(WorkerMsg::Tick {
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
                        MarketEvent::Disconnected(_) => { disconnected = true; }
                    }
                    _drained += 1;
                }
                Err(_) => break, // Empty
            }
        }

        // 2. Process Worker Signals
        loop {
            match signal_rx.try_recv() {
                Ok(msg) => {
                    app.handle_signal(msg);
                }
                Err(_) => break, // Empty
            }
        }

        // 3. Render
        if last_render.elapsed() >= render_interval {
            terminal.draw(|f| render_ui(f, &app))?;
            last_render = std::time::Instant::now();
            if event::poll(Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => { disconnected = true; }
                        KeyCode::Char('d') => { app.debug_mode = !app.debug_mode; }
                        _ => {}
                    }
                }
            }
        }

        if disconnected { break; }
        std::thread::yield_now();
    }

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    
    println!("\n═══ FINAL SUMMARY ═══");
    println!("Total P&L: ₹{:.2}", app.total_portfolio_pnl());
    println!("Trades: {} ({}W/{}L)", app.stats.trades_count, app.stats.winning_trades, app.stats.losing_trades);
    println!("Win Rate: {:.1}%", app.stats.win_rate());
    
    println!("\n┌Trade Log ({})─────────────────────────────────────────────────────────────────┐", app.trades.len());
    println!("│Time     Symbol    Side P&L      Reason                                         │");
    for trade in app.trades.iter() {
        let side_str = format!("{}", trade.side);
        let time_str = trade.exit_time.format("%H:%M:%S");
        println!("│{} {} {:<5} {:<8.2} {:<45} │", time_str, trade.symbol, side_str, trade.pnl, trade.exit_reason);
    }
    println!("└────────────────────────────────────────────────────────────────────────────────┘");

    Ok(())
}
