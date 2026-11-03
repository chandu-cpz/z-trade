pub mod chart;
pub mod state;

use std::collections::HashMap;
use std::time::Duration;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Tabs},
    Frame,
};

pub use chart::{
    render_stock_chart, NEO_ACCENT, NEO_BORDER, NEO_FOCUS_BORDER, NEO_GREEN, NEO_MUTED, NEO_RED,
};
pub use state::{ActivePane, ActiveTab, UiState};
use trade_core::types::PositionSide;
use trade_sim::PaperSimulationEngine;
use trade_strategy::StrategyEngine;

pub fn render_ui(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    strat_engine: &StrategyEngine,
    state: &UiState,
) {
    let size = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Navigation Header
            Constraint::Length(6), // Hero Portfolio Balance & Risk Card
            Constraint::Min(10),   // Active View Body (Dashboard / Chart / Signals / Analytics)
            Constraint::Length(2), // Bottom Status Bar / Vim status
        ])
        .split(size);

    render_header(frame, state, chunks[0]);
    render_hero_portfolio(frame, engine, chunks[1]);

    // Active Tab Content
    match state.active_tab {
        ActiveTab::Dashboard => render_dashboard_tab(frame, engine, strat_engine, state, chunks[2]),
        ActiveTab::Chart => render_fullscreen_chart(frame, strat_engine, state, chunks[2]),
        ActiveTab::Signals => render_signals_tab(frame, engine, state, chunks[2]),
        ActiveTab::Analytics => render_analytics_tab(frame, engine, state, chunks[2]),
    }

    render_footer(frame, engine, state, chunks[3]);

    if state.show_debug {
        render_debug_drawer(frame, engine, size);
    }

    if state.show_help {
        render_help_modal(frame, size);
    }
}

fn render_header(frame: &mut Frame, state: &UiState, area: Rect) {
    let titles = vec![
        Span::raw(" 1: Terminal "),
        Span::raw(" 2: Live Chart "),
        Span::raw(" 3: Signals Radar "),
        Span::raw(" 4: Analytics "),
    ];

    let tabs = Tabs::new(titles)
        .select(state.active_tab as usize)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(NEO_BORDER))
                .title(Line::from(vec![
                    Span::styled(" Z-TRADE ", Style::default().fg(Color::Black).bg(NEO_GREEN).add_modifier(Modifier::BOLD)),
                    Span::styled(" QUANT WORKSTATION ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(" [HFT / ULTRA-LOW LATENCY] ", Style::default().fg(NEO_ACCENT)),
                    Span::styled(" ● SFEED CONNECTED ", Style::default().fg(NEO_GREEN).add_modifier(Modifier::BOLD)),
                ])),
        )
        .style(Style::default().fg(NEO_MUTED))
        .highlight_style(
            Style::default()
                .fg(NEO_GREEN)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        )
        .divider(Span::styled("│", Style::default().fg(NEO_BORDER)));

    frame.render_widget(tabs, area);
}

fn render_hero_portfolio(frame: &mut Frame, engine: &PaperSimulationEngine, area: Rect) {
    let total_pnl = engine.total_portfolio_pnl();
    let unrealized = engine.unrealized_pnl();
    let realized = engine.stats.realized_pnl;
    let free = engine.free_capital();
    let locked = engine.locked_capital;
    let total_val = engine.total_value();
    let initial = engine.initial_capital;
    let total_return_pct = if initial > 0.0 { (total_pnl / initial) * 100.0 } else { 0.0 };

    let pnl_color = if total_pnl >= 0.0 { NEO_GREEN } else { NEO_RED };
    let pnl_sign = if total_pnl >= 0.0 { "+" } else { "" };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(24),
            Constraint::Percentage(24),
            Constraint::Percentage(24),
        ])
        .split(area);

    // Card 1: Total Portfolio Returns
    let c1 = Paragraph::new(vec![
        Line::from(Span::styled("TOTAL RETURNS", Style::default().fg(NEO_MUTED))),
        Line::from(vec![
            Span::styled(
                format!("₹{}{:.2}", pnl_sign, total_pnl),
                Style::default().fg(pnl_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("({}{:.2}%)", pnl_sign, total_return_pct),
                Style::default().fg(pnl_color),
            ),
        ]),
        Line::from(vec![
            Span::raw("Realized: "),
            Span::styled(format!("₹{:.2}", realized), Style::default().fg(if realized >= 0.0 { NEO_GREEN } else { NEO_RED })),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(NEO_BORDER)));
    frame.render_widget(c1, cols[0]);

    // Card 2: Current Net Portfolio Value
    let c2 = Paragraph::new(vec![
        Line::from(Span::styled("PORTFOLIO VALUATION", Style::default().fg(NEO_MUTED))),
        Line::from(Span::styled(
            format!("₹{:.2}", total_val),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::raw("Unrealized: "),
            Span::styled(format!("₹{:.2}", unrealized), Style::default().fg(if unrealized >= 0.0 { NEO_GREEN } else { NEO_RED })),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(NEO_BORDER)));
    frame.render_widget(c2, cols[1]);

    // Card 3: Margin & Free Capital
    let c3 = Paragraph::new(vec![
        Line::from(Span::styled("MARGIN / CAPITAL", Style::default().fg(NEO_MUTED))),
        Line::from(Span::styled(
            format!("₹{:.2}", locked),
            Style::default().fg(NEO_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::raw("Available Cash: "),
            Span::styled(format!("₹{:.0}", free), Style::default().fg(NEO_GREEN)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(NEO_BORDER)));
    frame.render_widget(c3, cols[2]);

    // Card 4: Risk & Trades
    let c4 = Paragraph::new(vec![
        Line::from(Span::styled("EXECUTION RISK", Style::default().fg(NEO_MUTED))),
        Line::from(vec![
            Span::raw("Positions: "),
            Span::styled(
                format!("{}/{}", engine.positions.len(), engine.config.portfolio.max_positions),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" │ Win: "),
            Span::styled(
                format!("{:.1}%", engine.stats.win_rate()),
                Style::default().fg(if engine.stats.win_rate() >= 50.0 { NEO_GREEN } else { NEO_RED }),
            ),
        ]),
        Line::from(vec![
            Span::raw("Circuit Limit: "),
            Span::styled(format!("₹{:.0}", engine.config.portfolio.max_loss), Style::default().fg(NEO_MUTED)),
            Span::raw(" │ Status: "),
            if engine.circuit_breaker_hit {
                Span::styled("TRIPPED ⚠", Style::default().fg(NEO_RED).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("NOMINAL", Style::default().fg(NEO_GREEN))
            },
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(NEO_BORDER)));
    frame.render_widget(c4, cols[3]);
}

fn render_dashboard_tab(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    strat_engine: &StrategyEngine,
    state: &UiState,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[0]);

    render_holdings_pane(frame, engine, state, left_chunks[0]);
    render_tradebook_pane(frame, engine, state, left_chunks[1]);

    // Right Split: Live Real-Time Interactive Chart
    let chart_symbol = state
        .selected_symbol
        .as_deref()
        .or_else(|| engine.positions.keys().next().map(|s| s.as_str()));

    let chart_data = chart_symbol.and_then(|sym| strat_engine.get_chart_data(sym));
    render_stock_chart(frame, chart_data.as_ref(), chunks[1]);
}

fn render_fullscreen_chart(
    frame: &mut Frame,
    strat_engine: &StrategyEngine,
    state: &UiState,
    area: Rect,
) {
    let chart_symbol = state
        .selected_symbol
        .as_deref()
        .or_else(|| strat_engine.trackers.keys().next().map(|s| s.as_str()));

    let chart_data = chart_symbol.and_then(|sym| strat_engine.get_chart_data(sym));
    render_stock_chart(frame, chart_data.as_ref(), area);
}

fn render_holdings_pane(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    state: &UiState,
    area: Rect,
) {
    let is_focused = state.active_pane == ActivePane::Holdings;
    let border_color = if is_focused { NEO_FOCUS_BORDER } else { NEO_BORDER };

    let mut positions: Vec<(&String, &trade_core::position::Position)> = engine.positions.iter().collect();
    positions.sort_by(|a, b| {
        b.1.unrealized_pnl()
            .partial_cmp(&a.1.unrealized_pnl())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows = vec![];
    for (i, (sym, pos)) in positions.iter().enumerate() {
        let is_selected = is_focused && i == state.holdings_selected;
        let pnl = pos.unrealized_pnl();
        let pnl_pct = pos.pnl_percent();
        let pnl_color = if pnl >= 0.0 { NEO_GREEN } else { NEO_RED };
        let side_color = if matches!(pos.side, PositionSide::Long) { Color::Cyan } else { Color::Yellow };

        let cursor = if is_selected { "❯ " } else { "  " };
        let sym_text = format!("{}{}", cursor, sym);

        let row_style = if is_selected {
            Style::default().bg(Color::Rgb(30, 41, 59)).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        rows.push(
            Row::new(vec![
                Cell::from(sym_text).style(if is_selected { Style::default().fg(NEO_GREEN) } else { Style::default() }),
                Cell::from(format!("{}", pos.side)).style(Style::default().fg(side_color)),
                Cell::from(format!("{}", pos.quantity)),
                Cell::from(format!("{:.2}", pos.entry_price)),
                Cell::from(format!("{:.2}", pos.current_price)),
                Cell::from(format!("₹{:.2}", pnl)).style(Style::default().fg(pnl_color)),
                Cell::from(format!("{:.2}%", pnl_pct)).style(Style::default().fg(pnl_color)),
                Cell::from(format!("{:.2}", pos.stop_loss)).style(Style::default().fg(NEO_MUTED)),
            ])
            .style(row_style),
        );
    }

    let title = format!(" Open Positions ({}) - [x] to Exit ", positions.len());
    let table = Table::new(
        rows,
        &[
            Constraint::Percentage(22),
            Constraint::Percentage(10),
            Constraint::Percentage(8),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(14),
            Constraint::Percentage(11),
            Constraint::Percentage(11),
        ],
    )
    .header(
        Row::new(vec!["Symbol", "Side", "Qty", "Entry", "LTP", "P&L", "P&L %", "StopLoss"])
            .style(Style::default().fg(NEO_MUTED).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(title),
    );

    frame.render_widget(table, area);
}

fn render_tradebook_pane(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    state: &UiState,
    area: Rect,
) {
    let is_focused = state.active_pane == ActivePane::Tradebook;
    let border_color = if is_focused { NEO_FOCUS_BORDER } else { NEO_BORDER };

    let mut rows = vec![];
    for (i, trade) in engine.trades.iter().rev().take(30).enumerate() {
        let is_selected = is_focused && i == state.tradebook_selected;
        let pnl = trade.net_pnl;
        let pnl_color = if pnl >= 0.0 { NEO_GREEN } else { NEO_RED };
        let cursor = if is_selected { "❯ " } else { "  " };

        let row_style = if is_selected {
            Style::default().bg(Color::Rgb(30, 41, 59)).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        rows.push(
            Row::new(vec![
                Cell::from(format!("{}{}", cursor, trade.exit_time.format("%H:%M:%S"))),
                Cell::from(trade.symbol.clone()),
                Cell::from(format!("{}", trade.side)),
                Cell::from(format!("{:.2}", trade.entry_price)),
                Cell::from(format!("{:.2}", trade.exit_price)),
                Cell::from(format!("₹{:.2}", pnl)).style(Style::default().fg(pnl_color)),
                Cell::from(trade.exit_reason.clone()).style(Style::default().fg(NEO_MUTED)),
            ])
            .style(row_style),
        );
    }

    let title = format!(" Executed Fills ({}) ", engine.trades.len());
    let table = Table::new(
        rows,
        &[
            Constraint::Percentage(16),
            Constraint::Percentage(22),
            Constraint::Percentage(10),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
        ],
    )
    .header(
        Row::new(vec!["Time", "Symbol", "Side", "Entry", "Exit", "Net P&L", "Reason"])
            .style(Style::default().fg(NEO_MUTED).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(title),
    );

    frame.render_widget(table, area);
}

fn render_signals_tab(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    state: &UiState,
    area: Rect,
) {
    let mut rows = vec![];
    for (i, sig) in engine.signal_feed.iter().rev().take(40).enumerate() {
        let is_selected = i == state.signals_selected;
        let side_color = if sig.side == PositionSide::Long { NEO_GREEN } else { NEO_RED };
        let cursor = if is_selected { "❯ " } else { "  " };

        let (status, status_style) = if sig.entered {
            ("EXECUTED", Style::default().fg(NEO_GREEN).add_modifier(Modifier::BOLD))
        } else {
            (
                sig.block_reason.as_deref().unwrap_or("FILTERED"),
                Style::default().fg(Color::Yellow),
            )
        };

        let row_style = if is_selected {
            Style::default().bg(Color::Rgb(30, 41, 59)).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        rows.push(
            Row::new(vec![
                Cell::from(format!("{}{}", cursor, sig.time.format("%H:%M:%S"))),
                Cell::from(sig.symbol.clone()),
                Cell::from(format!("{}", sig.side)).style(Style::default().fg(side_color)),
                Cell::from(format!("{}", sig.strategy)).style(Style::default().fg(NEO_ACCENT)),
                Cell::from(status).style(status_style),
            ])
            .style(row_style),
        );
    }

    let table = Table::new(
        rows,
        &[
            Constraint::Percentage(15),
            Constraint::Percentage(25),
            Constraint::Percentage(15),
            Constraint::Percentage(25),
            Constraint::Percentage(20),
        ],
    )
    .header(
        Row::new(vec!["Time", "Symbol", "Signal Side", "Strategy Engine", "Execution Status"])
            .style(Style::default().fg(NEO_MUTED).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(NEO_FOCUS_BORDER))
            .title(format!(" Impulse Screener & Alpha Signals ({} total) ", engine.signal_feed.len())),
    );

    frame.render_widget(table, area);
}

fn render_analytics_tab(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    _state: &UiState,
    area: Rect,
) {
    let mut stats: HashMap<String, (u32, u32, f64, f64)> = HashMap::new();
    for trade in &engine.trades {
        let key = trade.strategy.to_string();
        let entry = stats.entry(key).or_insert((0, 0, 0.0, 0.0));
        entry.0 += 1;
        if trade.net_pnl > 0.0 {
            entry.1 += 1;
        }
        entry.2 += trade.net_pnl;
        entry.3 += trade.transaction_costs;
    }

    let mut rows = vec![];
    let mut sorted: Vec<_> = stats.iter().collect();
    sorted.sort_by(|a, b| b.1 .2.partial_cmp(&a.1 .2).unwrap_or(std::cmp::Ordering::Equal));

    for (strat, (total, wins, net_pnl, fees)) in sorted {
        let wr = if *total > 0 { (*wins as f64 / *total as f64) * 100.0 } else { 0.0 };
        let pnl_color = if *net_pnl >= 0.0 { NEO_GREEN } else { NEO_RED };

        rows.push(Row::new(vec![
            Cell::from(strat.clone()).style(Style::default().fg(NEO_ACCENT).add_modifier(Modifier::BOLD)),
            Cell::from(format!("{}", total)),
            Cell::from(format!("{}", wins)),
            Cell::from(format!("{:.1}%", wr)).style(Style::default().fg(if wr >= 50.0 { NEO_GREEN } else { NEO_RED })),
            Cell::from(format!("₹{:.2}", net_pnl)).style(Style::default().fg(pnl_color)),
            Cell::from(format!("₹{:.2}", fees)).style(Style::default().fg(NEO_MUTED)),
        ]));
    }

    let table = Table::new(
        rows,
        &[
            Constraint::Percentage(25),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(
        Row::new(vec!["Strategy", "Trades", "Wins", "Win Rate", "Net Returns", "Taxes & Fees"])
            .style(Style::default().fg(NEO_MUTED).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(NEO_FOCUS_BORDER))
            .title(" Strategy Alpha & Execution Performance "),
    );

    frame.render_widget(table, area);
}

fn render_footer(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    state: &UiState,
    area: Rect,
) {
    let ticks_per_sec = {
        let now_inst = std::time::Instant::now();
        let window = Duration::from_secs(1);
        let count = engine
            .tick_times
            .iter()
            .rev()
            .take_while(|t| now_inst.duration_since(**t) < window)
            .count();
        count as f64
    };

    let status_text = if let Some((msg, ts)) = &state.status_message {
        if ts.elapsed() < Duration::from_secs(5) {
            Span::styled(format!(" ❯ {} ", msg), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(" [NORMAL] ", Style::default().fg(Color::Black).bg(NEO_GREEN).add_modifier(Modifier::BOLD))
        }
    } else {
        Span::styled(" [NORMAL] ", Style::default().fg(Color::Black).bg(NEO_GREEN).add_modifier(Modifier::BOLD))
    };

    let line = Line::from(vec![
        status_text,
        Span::raw("  "),
        Span::styled("j/k: nav", Style::default().fg(NEO_MUTED)),
        Span::raw(" │ "),
        Span::styled("Enter: chart", Style::default().fg(Color::Yellow)),
        Span::raw(" │ "),
        Span::styled("1..4: tabs", Style::default().fg(NEO_MUTED)),
        Span::raw(" │ "),
        Span::styled("x: exit pos", Style::default().fg(NEO_RED)),
        Span::raw(" │ "),
        Span::styled("X: panic all", Style::default().fg(NEO_RED).add_modifier(Modifier::BOLD)),
        Span::raw(" │ "),
        Span::styled("d: ticks", Style::default().fg(NEO_MUTED)),
        Span::raw(" │ "),
        Span::styled("?: help", Style::default().fg(Color::Cyan)),
        Span::raw(" │ "),
        Span::styled("q: quit", Style::default().fg(NEO_MUTED)),
        Span::raw(" │ Ticks: "),
        Span::styled(format!("{}", engine.tick_count), Style::default().fg(NEO_GREEN)),
        Span::raw(format!(" ({:.0}/s)", ticks_per_sec)),
    ]);

    let paragraph = Paragraph::new(line).alignment(Alignment::Left);
    frame.render_widget(paragraph, area);
}

fn render_help_modal(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(65, 65, area);
    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled("═══ Z-TRADE VIM NAVIGATION CHEAT SHEET ═══", Style::default().fg(NEO_GREEN).add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::from(vec![Span::styled("  j / Down     ", Style::default().fg(Color::Yellow)), Span::raw("Move selection down in active table")]),
        Line::from(vec![Span::styled("  k / Up       ", Style::default().fg(Color::Yellow)), Span::raw("Move selection up in active table")]),
        Line::from(vec![Span::styled("  Enter        ", Style::default().fg(Color::Yellow)), Span::raw("Open live technical chart for selected stock")]),
        Line::from(vec![Span::styled("  c            ", Style::default().fg(Color::Yellow)), Span::raw("Toggle fullscreen technical chart view")]),
        Line::from(vec![Span::styled("  g            ", Style::default().fg(Color::Yellow)), Span::raw("Jump to top of table")]),
        Line::from(vec![Span::styled("  G            ", Style::default().fg(Color::Yellow)), Span::raw("Jump to bottom of table")]),
        Line::from(vec![Span::styled("  Tab          ", Style::default().fg(Color::Yellow)), Span::raw("Cycle focus between Holdings & Tradebook")]),
        Line::from(vec![Span::styled("  h / l        ", Style::default().fg(Color::Yellow)), Span::raw("Cycle active tabs (Dashboard / Chart / Signals / Analytics)")]),
        Line::from(vec![Span::styled("  1, 2, 3, 4   ", Style::default().fg(Color::Yellow)), Span::raw("Direct tab jump")]),
        Line::raw(""),
        Line::from(Span::styled("─── TRADING ACTIONS & HOT-RELOAD ──────────", Style::default().fg(NEO_MUTED))),
        Line::from(vec![Span::styled("  x            ", Style::default().fg(NEO_RED)), Span::raw("Square off selected holding immediately at market")]),
        Line::from(vec![Span::styled("  X            ", Style::default().fg(NEO_RED).add_modifier(Modifier::BOLD)), Span::raw("PANIC: Close all open positions immediately")]),
        Line::from(vec![Span::styled("  d            ", Style::default().fg(Color::Cyan)), Span::raw("Toggle live tick stream drawer")]),
        Line::from(vec![Span::styled("  ? or /       ", Style::default().fg(Color::Cyan)), Span::raw("Toggle this help modal")]),
        Line::from(vec![Span::styled("  q / Esc      ", Style::default().fg(NEO_MUTED)), Span::raw("Close popup or quit application")]),
        Line::raw(""),
        Line::from(Span::styled("⚡ Strategies & risk hot-reload automatically when editing configs/default.toml", Style::default().fg(NEO_GREEN))),
    ];

    let block = Block::default()
        .title(" Vim Shortcuts & Commands ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(NEO_GREEN));

    let paragraph = Paragraph::new(help_text).block(block);
    frame.render_widget(paragraph, popup_area);
}

fn render_debug_drawer(frame: &mut Frame, engine: &PaperSimulationEngine, area: Rect) {
    let drawer_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area)[1];

    frame.render_widget(Clear, drawer_area);

    let visible = drawer_area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = engine
        .debug_ticks
        .iter()
        .rev()
        .take(visible)
        .rev()
        .map(|s| Line::from(Span::styled(s.clone(), Style::default().fg(Color::Green))))
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(format!(" 🔍 LIVE TICK STREAM ([d] to close) - {} ticks buffered ", engine.debug_ticks.len()));

    let p = Paragraph::new(lines).block(block);
    frame.render_widget(p, drawer_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
