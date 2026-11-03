use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};
use std::collections::HashMap;

use crate::models::{Position, PositionSide};
use crate::sim::PaperEngine;

pub fn render_ui(frame: &mut Frame, engine: &PaperEngine) {
    let size = frame.area();

    if engine.debug_mode {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(0)])
            .split(size);
        render_portfolio_summary(frame, engine, main_chunks[0]);
        render_debug_ticks(frame, engine, main_chunks[1]);
    } else {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(0)])
            .split(size);

        render_portfolio_summary(frame, engine, main_chunks[0]);

        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(main_chunks[1]);

        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(content_chunks[0]);

        render_holdings(frame, engine, left_chunks[0]);
        render_tradebook(frame, engine, left_chunks[1]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(content_chunks[1]);

        render_signal_feed(frame, engine, right_chunks[0]);
        render_strategy_stats(frame, engine, right_chunks[1]);
    }
}

pub fn render_portfolio_summary(frame: &mut Frame, engine: &PaperEngine, area: Rect) {
    let total_pnl = engine.total_portfolio_pnl();
    let unrealized = engine.unrealized_pnl();
    let realized = engine.stats.realized_pnl;
    let free = engine.free_capital();
    let locked = engine.locked_capital;
    let total_val = engine.total_value();
    let win_rate = engine.stats.win_rate();
    let max_pos = engine.config.max_positions;
    let max_loss = engine.config.max_loss;

    let pnl_color = if total_pnl >= 0.0 { Color::Green } else { Color::Red };

    let status = if engine.circuit_breaker_hit {
        Span::styled(
            " ⚠ CIRCUIT BREAKER ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ✓ ACTIVE ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )
    };

    let paper_badge = Span::styled(
        " [SIMULATION / PAPER ONLY] ",
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
    );

    let ticks_per_sec = {
        let now_inst = std::time::Instant::now();
        let window = std::time::Duration::from_secs(1);
        let count = engine
            .tick_times
            .iter()
            .rev()
            .take_while(|t| now_inst.duration_since(**t) < window)
            .count();
        count as f64
    };

    let text = vec![
        Line::from(vec![
            Span::styled(
                "═══ Z-TRADE PORTFOLIO ═══",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            status,
            Span::raw("  "),
            paper_badge,
        ]),
        Line::from(vec![
            Span::raw("Total P&L: "),
            Span::styled(
                format!("₹{:.2}", total_pnl),
                Style::default().fg(pnl_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  Realized: "),
            Span::styled(
                format!("₹{:.2}", realized),
                Style::default().fg(if realized >= 0.0 { Color::Green } else { Color::Red }),
            ),
            Span::raw("  |  Unrealized: "),
            Span::styled(
                format!("₹{:.2}", unrealized),
                Style::default().fg(if unrealized >= 0.0 { Color::Green } else { Color::Yellow }),
            ),
        ]),
        Line::from(vec![
            Span::raw("Free: "),
            Span::styled(
                format!("₹{:.0}", free),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  Locked: "),
            Span::styled(format!("₹{:.0}", locked), Style::default().fg(Color::Yellow)),
            Span::raw("  |  Portfolio: "),
            Span::styled(format!("₹{:.0}", total_val), Style::default().fg(Color::Cyan)),
            Span::raw("  |  Trades: "),
            Span::styled(format!("{}", engine.stats.trades_count), Style::default()),
            Span::raw("  |  Win: "),
            Span::styled(
                format!("{:.1}%", win_rate),
                Style::default().fg(if win_rate >= 50.0 { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::raw("Positions: "),
            Span::styled(
                format!("{}/{}", engine.positions.len(), max_pos),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  "),
            Span::raw(format!("MaxLoss: ₹{:.0}", max_loss)),
            Span::raw("  |  Ticks: "),
            Span::styled(format!("{}", engine.tick_count), Style::default().fg(Color::Green)),
            Span::raw(" ("),
            Span::styled(format!("{:.0}/s", ticks_per_sec), Style::default().fg(Color::Yellow)),
            Span::raw(")  "),
            if engine.debug_mode {
                Span::styled("[d] DEBUG ON", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("[d] debug", Style::default().fg(Color::DarkGray))
            },
            Span::raw("  [q] quit"),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Portfolio Summary"));
    frame.render_widget(paragraph, area);
}

pub fn render_holdings(frame: &mut Frame, engine: &PaperEngine, area: Rect) {
    let mut rows = vec![];
    let mut positions: Vec<(&String, &Position)> = engine.positions.iter().collect();
    positions.sort_by(|a, b| {
        b.1.unrealized_pnl()
            .partial_cmp(&a.1.unrealized_pnl())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for (sym, pos) in &positions {
        let pnl = pos.unrealized_pnl();
        let pnl_pct = pos.pnl_percent();
        let locked = pos.entry_price * (pos.quantity as f64);

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
            Constraint::Percentage(14),
            Constraint::Percentage(9),
            Constraint::Percentage(7),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(12),
            Constraint::Percentage(16),
            Constraint::Percentage(16),
        ],
    )
    .header(
        Row::new(vec!["Symb", "Side", "Qty", "Entry", "LTP", "Locked", "P&L", "P&L%"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Holdings ({}/{})", engine.positions.len(), engine.config.max_positions)),
    );

    frame.render_widget(table, area);
}

pub fn render_tradebook(frame: &mut Frame, engine: &PaperEngine, area: Rect) {
    let mut rows = vec![];
    for trade in engine.trades.iter().rev().take(20) {
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
            Constraint::Percentage(12),
            Constraint::Percentage(18),
            Constraint::Percentage(10),
            Constraint::Percentage(15),
            Constraint::Percentage(12),
            Constraint::Percentage(15),
            Constraint::Percentage(18),
        ],
    )
    .header(
        Row::new(vec!["Time", "Symbol", "Side", "Entry", "Exit", "P&L", "Reason"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Trade Book (Last 20 of {})", engine.trades.len())),
    );

    frame.render_widget(table, area);
}

pub fn render_signal_feed(frame: &mut Frame, engine: &PaperEngine, area: Rect) {
    let visible = area.height.saturating_sub(3) as usize;
    let header = Line::from(vec![
        Span::styled(format!("{:<8}", "Time"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<10}", "Symbol"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<5}", "Side"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<9}", "Strategy"), Style::default().fg(Color::DarkGray)),
        Span::styled("Status".to_string(), Style::default().fg(Color::DarkGray)),
    ]);

    let mut lines: Vec<Line> = vec![header];
    for sig in engine.signal_feed.iter().rev().take(visible) {
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
            Span::styled(format!("{:<10}", sym), Style::default().fg(Color::White)),
            Span::styled(format!("{:<5}", side_str), Style::default().fg(side_color)),
            Span::styled(format!("{:<9}", sig.strategy), Style::default().fg(Color::Cyan)),
            Span::styled(status_str, Style::default().fg(status_color)),
        ]));
    }

    let block = Block::default()
        .title(format!(" Signal Feed ({} total) ", engine.signal_feed.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_strategy_stats(frame: &mut Frame, engine: &PaperEngine, area: Rect) {
    let mut stats: HashMap<String, (u32, u32, f64)> = HashMap::new();
    for trade in &engine.trades {
        let key = trade.strategy.to_string();
        let entry = stats.entry(key).or_insert((0, 0, 0.0));
        entry.0 += 1;
        if trade.pnl > 0.0 {
            entry.1 += 1;
        }
        entry.2 += trade.pnl;
    }

    let header = Line::from(vec![
        Span::styled(format!("{:<9}", "Strategy"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>6}", "Trades"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>7}", "WR%"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>9}", "Total P&L"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:>8}", "Avg P&L"), Style::default().fg(Color::DarkGray)),
    ]);

    let mut lines: Vec<Line> = vec![header];
    let mut sorted: Vec<_> = stats.iter().collect();
    sorted.sort_by(|a, b| b.1 .2.partial_cmp(&a.1 .2).unwrap_or(std::cmp::Ordering::Equal));

    for (strat, (total, wins, total_pnl)) in &sorted {
        let wr = if *total > 0 { (*wins as f64) / (*total as f64) * 100.0 } else { 0.0 };
        let avg_pnl = if *total > 0 { total_pnl / (*total as f64) } else { 0.0 };
        let pnl_color = if *total_pnl >= 0.0 { Color::Green } else { Color::Red };
        let wr_color = if wr >= 50.0 { Color::Green } else if wr >= 40.0 { Color::Yellow } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<9}", strat), Style::default().fg(Color::Cyan)),
            Span::styled(format!("{:>6}", total), Style::default().fg(Color::White)),
            Span::styled(format!("{:>6.1}%", wr), Style::default().fg(wr_color)),
            Span::styled(format!("{:>9.2}", total_pnl), Style::default().fg(pnl_color)),
            Span::styled(format!("{:>8.2}", avg_pnl), Style::default().fg(pnl_color)),
        ]));
    }

    let block = Block::default()
        .title(" Strategy Stats ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_debug_ticks(frame: &mut Frame, engine: &PaperEngine, area: Rect) {
    let visible = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = engine
        .debug_ticks
        .iter()
        .rev()
        .take(visible)
        .rev()
        .map(|s| {
            let color = if s.contains('▲') {
                Color::Green
            } else if s.contains('▼') {
                Color::Red
            } else {
                Color::DarkGray
            };
            Line::from(Span::styled(s.clone(), Style::default().fg(color)))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("🔍 LIVE TICKS [d] to close | {} ticks buffered", engine.debug_ticks.len()))
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
