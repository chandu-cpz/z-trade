pub mod chart;
pub mod order_modal;
pub mod state;


use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Tabs},
    Frame,
};

pub use chart::{
    render_stock_view, KITE_BLUE, KITE_BORDER, KITE_GREEN, KITE_MUTED, KITE_RED,
};
pub use order_modal::{render_order_modal, OrderModalState, ProductType};
pub use state::{ActivePane, BottomTab, UiState};
pub use trade_core::store::PendingOrder;
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
            Constraint::Length(3), // Top Navigation Bar & Market Tickers
            Constraint::Min(10),   // Main Split (Watchlist on Left, Stock & Tabs on Right)
            Constraint::Length(1), // Bottom 1-line Vim Status & Shortcuts Bar
        ])
        .split(size);

    render_top_bar(frame, engine, state, chunks[0]);
    render_main_split(frame, engine, strat_engine, state, chunks[1]);
    render_bottom_status_bar(frame, state, chunks[2]);

    if state.show_help {
        render_help_modal(frame, size);
    }

    if state.order_modal.is_open {
        render_order_modal(frame, &state.order_modal, engine, size);
    }
}

fn render_top_bar(frame: &mut Frame, engine: &PaperSimulationEngine, _state: &UiState, area: Rect) {
    let tot_pnl = engine.total_portfolio_pnl();
    let pnl_color = if tot_pnl >= 0.0 { KITE_GREEN } else { KITE_RED };
    let pnl_sign = if tot_pnl >= 0.0 { "+₹" } else { "-₹" };

    let top_line = Line::from(vec![
        Span::styled(
            " Z-TRADE ",
            Style::default().fg(Color::Black).bg(KITE_BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  KITE / GROWW STYLE  ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " [PAPER TRADING] ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │  NIFTY 50: "),
        Span::styled("22,450.30 ▲ +0.41%", Style::default().fg(KITE_GREEN).add_modifier(Modifier::BOLD)),
        Span::raw("  BANKNIFTY: "),
        Span::styled("48,150.20 ▼ -0.14%", Style::default().fg(KITE_RED).add_modifier(Modifier::BOLD)),
        Span::raw("  │  Cash: "),
        Span::styled(format!("₹{:.2}", engine.free_capital()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  Net P&L: "),
        Span::styled(format!("{}{:.2}", pnl_sign, tot_pnl.abs()), Style::default().fg(pnl_color).add_modifier(Modifier::BOLD)),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(KITE_BORDER));
    frame.render_widget(Paragraph::new(top_line).block(block), area);
}

fn render_main_split(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    strat_engine: &StrategyEngine,
    state: &UiState,
    area: Rect,
) {
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(34), // Watchlist on Left (34 cols)
            Constraint::Min(40),    // Right Panel: Stock View (Chart/Depth) + Bottom Tabs
        ])
        .split(area);

    // Left: Watchlist
    render_watchlist(frame, engine, strat_engine, state, h_chunks[0]);

    // Right: Stock Chart + Kite Bottom Tabs (Positions/Orders/Funds)
    let r_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(58), // Stock Header, Candlestick Chart, 5-Depth
            Constraint::Percentage(42), // Bottom Tabs (Positions / Orders / Funds)
        ])
        .split(h_chunks[1]);

    let chart_data = strat_engine.get_chart_data(&state.selected_symbol);
    let tf_label = match state.chart_timeframe {
        300 => "5m",
        900 => "15m",
        _ => "1m",
    };
    render_stock_view(frame, chart_data.as_ref(), state.show_depth, tf_label, r_chunks[0]);

    render_bottom_tabs(frame, engine, state, r_chunks[1]);
}

fn render_watchlist(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    strat_engine: &StrategyEngine,
    state: &UiState,
    area: Rect,
) {
    let is_focused = state.active_pane == ActivePane::Watchlist;
    let border_color = if is_focused { KITE_BLUE } else { KITE_BORDER };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tabs: NIFTY | F&O | HOLDINGS
            Constraint::Length(3), // Search bar (/ to search)
            Constraint::Min(6),    // Stock table
            Constraint::Length(2), // Quick action hints
        ])
        .split(area);

    // 1. Watchlist Tabs
    let tab_titles = vec![
        Span::raw(" [1: NIFTY] "),
        Span::raw(" [2: F&O] "),
        Span::raw(" [3: HOLDINGS] "),
    ];
    let tabs = Tabs::new(tab_titles)
        .select(state.watchlist_tab)
        .style(Style::default().fg(KITE_MUTED))
        .highlight_style(Style::default().fg(KITE_BLUE).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .title(" Marketwatch "),
        );
    frame.render_widget(tabs, chunks[0]);

    // 2. Search Bar
    let search_style = if state.is_searching {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let search_content = if state.is_searching {
        format!(" / {}█", state.search_query)
    } else if state.search_query.is_empty() {
        " / Press '/' to search symbols...".to_string()
    } else {
        format!(" / Filter: {}", state.search_query)
    };
    let search_block = Paragraph::new(search_content)
        .style(search_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(if state.is_searching { Color::Yellow } else { KITE_BORDER })),
        );
    frame.render_widget(search_block, chunks[1]);

    // 3. Stock Rows
    let filtered_stocks = get_watchlist_stocks(engine, state);
    if filtered_stocks.is_empty() {
        let empty_msg = if state.watchlist_tab == 2 {
            "No active holdings.\nPress [b] to Buy stock."
        } else {
            "No matching symbols."
        };
        let p = Paragraph::new(empty_msg)
            .style(Style::default().fg(KITE_MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                    .border_style(Style::default().fg(border_color)),
            );
        frame.render_widget(p, chunks[2]);
    } else {
        let rows: Vec<Row> = filtered_stocks
            .iter()
            .enumerate()
            .map(|(idx, sym)| {
                let tracker = strat_engine.trackers.get(sym);
                let price = tracker.map(|t| t.current_price).unwrap_or(100.0);
                let first_p = tracker.and_then(|t| t.bar_aggregator.all_bars().first().map(|b| b.open)).unwrap_or(price);
                let chg_pct = if first_p > 0.0 { ((price - first_p) / first_p) * 100.0 } else { 0.0 };

                let chg_color = if chg_pct >= 0.0 { KITE_GREEN } else { KITE_RED };
                let sign = if chg_pct >= 0.0 { "+" } else { "" };

                let is_sel = is_focused && idx == state.watchlist_selected;
                let row_style = if is_sel {
                    Style::default().fg(Color::White).bg(Color::Rgb(30, 41, 59)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                Row::new(vec![
                    Cell::from(Span::styled(sym.replace("-EQ", ""), row_style)),
                    Cell::from(Span::styled(format!("₹{:.2}", price), row_style)),
                    Cell::from(Span::styled(format!("{}{:.2}%", sign, chg_pct), Style::default().fg(chg_color))),
                ])
                .style(row_style)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(9),
            ],
        )
        .header(
            Row::new(vec!["SYMBOL", "LTP", "CHG%"])
                .style(Style::default().fg(KITE_MUTED).add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                .border_style(Style::default().fg(border_color)),
        );
        frame.render_widget(table, chunks[2]);
    }

    // 4. Quick Action Hints
    let hints = Line::from(vec![
        Span::styled(" [b] BUY ", Style::default().fg(KITE_BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" [s] SELL ", Style::default().fg(Color::Rgb(255, 87, 34)).add_modifier(Modifier::BOLD)),
        Span::styled(" [d] DEPTH ", Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[3]);
}

fn render_bottom_tabs(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    state: &UiState,
    area: Rect,
) {
    let is_focused = state.active_pane == ActivePane::BottomTabs;
    let border_color = if is_focused { KITE_BLUE } else { KITE_BORDER };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab switcher: [1: Positions] [2: Orders] [3: Funds]
            Constraint::Min(4),    // Active Table Content
        ])
        .split(area);

    let titles = vec![
        Span::raw(format!(" [1: POSITIONS ({})] ", engine.positions.len())),
        Span::raw(format!(" [2: ORDERS ({})] ", engine.pending_orders.len() + engine.trades.len())),
        Span::raw(" [3: FUNDS & MARGINS] "),
    ];

    let tabs = Tabs::new(titles)
        .select(state.bottom_tab as usize)
        .style(Style::default().fg(KITE_MUTED))
        .highlight_style(Style::default().fg(KITE_BLUE).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color)),
        );
    frame.render_widget(tabs, chunks[0]);

    match state.bottom_tab {
        BottomTab::Positions => render_positions_table(frame, engine, state, chunks[1], border_color),
        BottomTab::Orders => render_orders_table(frame, engine, state, chunks[1], border_color),
        BottomTab::Funds => render_funds_view(frame, engine, chunks[1], border_color),
    }
}

fn render_positions_table(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    state: &UiState,
    area: Rect,
    border_color: Color,
) {
    if engine.positions.is_empty() {
        let p = Paragraph::new("No open positions. Press [b] to Buy or [s] to Sell any stock in the Watchlist.")
            .style(Style::default().fg(KITE_MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                    .border_style(Style::default().fg(border_color)),
            );
        frame.render_widget(p, area);
        return;
    }

    let is_focused = state.active_pane == ActivePane::BottomTabs;
    let mut total_unrealized = 0.0;

    let rows: Vec<Row> = engine
        .positions
        .values()
        .enumerate()
        .map(|(idx, pos)| {
            let pnl = pos.unrealized_pnl();
            total_unrealized += pnl;
            let pnl_pct = pos.pnl_percent();
            let pnl_color = if pnl >= 0.0 { KITE_GREEN } else { KITE_RED };
            let sign = if pnl >= 0.0 { "+₹" } else { "-₹" };

            let is_sel = is_focused && idx == state.positions_selected;
            let row_style = if is_sel {
                Style::default().fg(Color::White).bg(Color::Rgb(30, 41, 59)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Cell::from(Span::styled("MIS", Style::default().fg(Color::Cyan))),
                Cell::from(Span::styled(&pos.symbol, row_style)),
                Cell::from(Span::styled(format!("{}", pos.quantity), row_style)),
                Cell::from(Span::styled(format!("₹{:.2}", pos.entry_price), row_style)),
                Cell::from(Span::styled(format!("₹{:.2}", pos.current_price), row_style)),
                Cell::from(Span::styled(format!("{}{:.2}", sign, pnl.abs()), Style::default().fg(pnl_color).add_modifier(Modifier::BOLD))),
                Cell::from(Span::styled(format!("{:.2}%", pnl_pct), Style::default().fg(pnl_color))),
                Cell::from(Span::styled(" [x] Square-off ", Style::default().fg(KITE_RED))),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),  // Product
            Constraint::Length(14), // Symbol
            Constraint::Length(8),  // Qty
            Constraint::Length(12), // Avg
            Constraint::Length(12), // LTP
            Constraint::Length(14), // P&L
            Constraint::Length(10), // P&L %
            Constraint::Length(18), // Action
        ],
    )
    .header(
        Row::new(vec!["PRODUCT", "SYMBOL", "QTY", "AVG PRICE", "LTP", "P&L (₹)", "CHG%", "ACTION"])
            .style(Style::default().fg(KITE_MUTED).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(border_color)),
    );
    frame.render_widget(table, area);
}

fn render_orders_table(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    _state: &UiState,
    area: Rect,
    border_color: Color,
) {
    let mut rows = Vec::new();

    // 1. Pending Orders
    for ord in &engine.pending_orders {
        let side_color = if ord.side == PositionSide::Long { KITE_BLUE } else { Color::Rgb(255, 87, 34) };
        let side_str = if ord.side == PositionSide::Long { "BUY" } else { "SELL" };
        let price_str = ord.limit_price.map(|p| format!("₹{:.2}", p)).unwrap_or_else(|| "MKT".to_string());

        rows.push(Row::new(vec![
            Cell::from(Span::raw(ord.placed_at.format("%H:%M:%S").to_string())),
            Cell::from(Span::raw(&ord.id)),
            Cell::from(Span::styled(side_str, Style::default().fg(side_color).add_modifier(Modifier::BOLD))),
            Cell::from(Span::raw("MIS")),
            Cell::from(Span::raw(&ord.order_type)),
            Cell::from(Span::raw(format!("{}", ord.quantity))),
            Cell::from(Span::raw(price_str)),
            Cell::from(Span::styled("OPEN", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        ]));
    }

    // 2. Executed Trades (Last 10)
    for trade in engine.trades.iter().rev().take(10) {
        let side_color = if trade.side == PositionSide::Long { KITE_BLUE } else { Color::Rgb(255, 87, 34) };
        let side_str = if trade.side == PositionSide::Long { "BUY" } else { "SELL" };

        rows.push(Row::new(vec![
            Cell::from(Span::raw(trade.exit_time.format("%H:%M:%S").to_string())),
            Cell::from(Span::raw("EXEC")),
            Cell::from(Span::styled(side_str, Style::default().fg(side_color))),
            Cell::from(Span::raw("MIS")),
            Cell::from(Span::raw("MKT")),
            Cell::from(Span::raw(format!("{}", trade.quantity))),
            Cell::from(Span::raw(format!("₹{:.2}", trade.exit_price))),
            Cell::from(Span::styled("COMPLETE", Style::default().fg(KITE_GREEN))),
        ]));
    }

    if rows.is_empty() {
        let p = Paragraph::new("No orders placed yet. Press [b] to Buy or [s] to Sell.")
            .style(Style::default().fg(KITE_MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                    .border_style(Style::default().fg(border_color)),
            );
        frame.render_widget(p, area);
        return;
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(10), // Time
            Constraint::Length(14), // Order ID
            Constraint::Length(6),  // Side
            Constraint::Length(6),  // Product
            Constraint::Length(8),  // Type
            Constraint::Length(8),  // Qty
            Constraint::Length(12), // Price
            Constraint::Length(12), // Status
        ],
    )
    .header(
        Row::new(vec!["TIME", "ORDER ID", "SIDE", "PROD", "TYPE", "QTY", "PRICE", "STATUS"])
            .style(Style::default().fg(KITE_MUTED).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(border_color)),
    );
    frame.render_widget(table, area);
}

fn render_funds_view(
    frame: &mut Frame,
    engine: &PaperSimulationEngine,
    area: Rect,
    border_color: Color,
) {
    let tot_pnl = engine.total_portfolio_pnl();
    let pnl_color = if tot_pnl >= 0.0 { KITE_GREEN } else { KITE_RED };

    let lines = vec![
        Line::from(vec![
            Span::styled(" Available Cash:       ", Style::default().fg(KITE_MUTED)),
            Span::styled(format!("₹{:.2}", engine.free_capital()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("     │  Opening Balance:       ", Style::default().fg(KITE_MUTED)),
            Span::styled(format!("₹{:.2}", engine.initial_capital), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" Margin Used (MIS):    ", Style::default().fg(KITE_MUTED)),
            Span::styled(format!("₹{:.2}", engine.locked_capital), Style::default().fg(Color::Yellow)),
            Span::styled("     │  Realized Day P&L:      ", Style::default().fg(KITE_MUTED)),
            Span::styled(format!("₹{:.2}", engine.stats.realized_pnl), Style::default().fg(pnl_color)),
        ]),
        Line::from(vec![
            Span::styled(" Total Portfolio Value:", Style::default().fg(KITE_MUTED)),
            Span::styled(format!("₹{:.2}", engine.total_value()), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("     │  MIS Intraday Leverage: ", Style::default().fg(KITE_MUTED)),
            Span::styled("5x (20% Margin)", Style::default().fg(KITE_GREEN)),
        ]),
    ];

    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(border_color)),
    );
    frame.render_widget(p, area);
}

fn render_bottom_status_bar(frame: &mut Frame, state: &UiState, area: Rect) {
    let msg_span = if let Some((msg, _)) = &state.status_message {
        Span::styled(format!(" 🔔 {} ", msg), Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(
            " [j/k] Nav  [/] Search  [b] Buy  [s] Sell  [x] Square-off  [d] Depth  [1-3] Tabs  [Tab] Pane  [?] Help  [q] Quit ",
            Style::default().fg(KITE_MUTED),
        )
    };

    let p = Paragraph::new(Line::from(vec![msg_span]));
    frame.render_widget(p, area);
}

fn render_help_modal(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 60, area);
    frame.render_widget(Clear, popup);

    let help_text = vec![
        Line::from(Span::styled(" ZERODHA KITE / GROWW STYLE TERMINAL KEYBINDINGS ", Style::default().fg(Color::White).bg(KITE_BLUE).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![Span::styled(" Watchlist Navigation: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("   j / Down         : Move down watchlist"),
        Line::from("   k / Up           : Move up watchlist"),
        Line::from("   /                : Open instant Search (type symbol, Enter to select, Esc to clear)"),
        Line::from("   Enter            : Focus highlighted stock and update Chart"),
        Line::from(""),
        Line::from(vec![Span::styled(" Rapid Order Execution (Paper OMS): ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("   b                : Open BUY order slip (Blue modal)"),
        Line::from("   s                : Open SELL order slip (Orange modal)"),
        Line::from("   m / c (in modal) : Toggle MIS (Intraday) vs CNC (Delivery)"),
        Line::from("   t (in modal)     : Toggle MARKET vs LIMIT vs SL"),
        Line::from("   Left/Right       : Adjust quantity"),
        Line::from("   Enter (in modal) : Submit paper order"),
        Line::from("   Esc              : Cancel / Close modal"),
        Line::from(""),
        Line::from(vec![Span::styled(" Positions & Views: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("   x                : Square-off highlighted open position at market"),
        Line::from("   d                : Toggle 5-Depth Market Ladder alongside Chart"),
        Line::from("   1, 2, 3          : Switch bottom tab (Positions, Orders, Funds)"),
        Line::from("   Tab / h / l      : Switch active pane (Watchlist <-> Chart <-> Bottom Tabs)"),
        Line::from("   q                : Clean exit and save session"),
        Line::from(""),
        Line::from(Span::styled(" Press [?] or [Esc] to close this help window. ", Style::default().fg(Color::Cyan))),
    ];

    let p = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(KITE_BLUE))
            .title(" Keyboard Shortcuts (Vim / Kite) "),
    );
    frame.render_widget(p, popup);
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

pub fn get_watchlist_stocks(engine: &PaperSimulationEngine, state: &UiState) -> Vec<String> {
    let base_stocks: Vec<String> = match state.watchlist_tab {
        2 => {
            let mut pos_syms: Vec<String> = engine.positions.keys().cloned().collect();
            pos_syms.sort();
            pos_syms
        }
        1 => {
            vec![
                "RELIANCE".to_string(),
                "HDFCBANK".to_string(),
                "TATAMOTORS".to_string(),
                "ICICIBANK".to_string(),
                "INFY".to_string(),
                "SBIN".to_string(),
                "KOTAKBANK".to_string(),
                "BHARTIARTL".to_string(),
            ]
        }
        _ => engine.watchlist.clone(),
    };

    if state.search_query.is_empty() {
        base_stocks
    } else {
        let q = state.search_query.to_uppercase();
        base_stocks.into_iter().filter(|s| s.to_uppercase().contains(&q)).collect()
    }
}
