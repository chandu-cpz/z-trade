use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, BorderType, Borders, Chart, Dataset, GraphType, Paragraph},
    Frame,
};
use trade_strategy::ChartData;

pub const NEO_GREEN: Color = Color::Rgb(0, 210, 144);
pub const NEO_RED: Color = Color::Rgb(235, 87, 87);
pub const NEO_ACCENT: Color = Color::Rgb(99, 102, 241);
pub const NEO_MUTED: Color = Color::Rgb(148, 163, 184);
pub const NEO_BORDER: Color = Color::Rgb(51, 65, 85);
pub const NEO_FOCUS_BORDER: Color = Color::Rgb(0, 210, 144);

pub fn render_stock_chart(frame: &mut Frame, chart_data: Option<&ChartData>, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Symbol & Live Metrics Header
            Constraint::Min(8),    // Braille Canvas Chart
            Constraint::Length(4), // Order Book Imbalance & Depth Ladder
        ])
        .split(area);

    guard_render_chart(frame, chart_data, chunks);
}

fn guard_render_chart(frame: &mut Frame, data_opt: Option<&ChartData>, chunks: std::rc::Rc<[Rect]>) {
    let Some(data) = data_opt else {
        let p = Paragraph::new("No active stock selected. Use [j]/[k] in Holdings or Signals to view live charts.")
            .style(Style::default().fg(NEO_MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(NEO_BORDER))
                    .title(" Live Real-Time Chart "),
            );
        frame.render_widget(p, chunks[1]);
        return;
    };

    // 1. Metric Header
    let rsi_color = if data.rsi_val > 70.0 {
        NEO_RED
    } else if data.rsi_val < 30.0 {
        NEO_GREEN
    } else {
        Color::Cyan
    };

    let spread = (data.best_ask - data.best_bid).max(0.0);
    let spread_pct = if data.current_price > 0.0 {
        (spread / data.current_price) * 100.0
    } else {
        0.0
    };

    let header_line = Line::from(vec![
        Span::styled(format!(" {} ", data.symbol), Style::default().fg(Color::Black).bg(NEO_GREEN).add_modifier(Modifier::BOLD)),
        Span::raw("  LTP: "),
        Span::styled(format!("₹{:.2}", data.current_price), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("  │  VWAP: "),
        Span::styled(format!("₹{:.2}", data.vwap_val), Style::default().fg(Color::Yellow)),
        Span::raw("  │  EMA(13): "),
        Span::styled(format!("₹{:.2}", data.ema_val), Style::default().fg(Color::Magenta)),
        Span::raw("  │  RSI(14): "),
        Span::styled(format!("{:.1}", data.rsi_val), Style::default().fg(rsi_color).add_modifier(Modifier::BOLD)),
        Span::raw("  │  Spread: "),
        Span::styled(format!("{:.3}% (₹{:.2})", spread_pct, spread), Style::default().fg(NEO_MUTED)),
    ]);

    let header = Paragraph::new(header_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(NEO_BORDER)),
    );
    frame.render_widget(header, chunks[0]);

    // 2. High-Resolution Braille Chart
    let price_pts: &[(f64, f64)] = &data.price_points;
    let n_pts = price_pts.len() as f64;
    let x_bounds = [0.0, n_pts.max(10.0)];
    let y_bounds = [data.min_price, data.max_price];

    // Build constant reference lines for VWAP and EMA
    let vwap_line: Vec<(f64, f64)> = if n_pts > 0.0 {
        vec![(0.0, data.vwap_val), (n_pts, data.vwap_val)]
    } else {
        vec![]
    };

    let ema_line: Vec<(f64, f64)> = if n_pts > 0.0 {
        vec![(0.0, data.ema_val), (n_pts, data.ema_val)]
    } else {
        vec![]
    };

    let datasets = vec![
        Dataset::default()
            .name("Price (Braille)")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(NEO_GREEN).add_modifier(Modifier::BOLD))
            .data(price_pts),
        Dataset::default()
            .name("VWAP")
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Yellow))
            .data(&vwap_line),
        Dataset::default()
            .name("EMA(13)")
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Magenta))
            .data(&ema_line),
    ];

    let mid_price = (data.min_price + data.max_price) / 2.0;
    let y_labels = vec![
        Span::raw(format!("{:.1}", data.min_price)),
        Span::raw(format!("{:.1}", mid_price)),
        Span::raw(format!("{:.1}", data.max_price)),
    ];

    let x_labels = vec![
        Span::raw("t-60"),
        Span::raw("t-30"),
        Span::raw("Now"),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(format!(" {} Live Price Action (Sub-character Braille Canvas) ", data.symbol))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(NEO_FOCUS_BORDER)),
        )
        .x_axis(
            Axis::default()
                .title(Span::styled("Time Progression", Style::default().fg(NEO_MUTED)))
                .style(Style::default().fg(NEO_BORDER))
                .bounds(x_bounds)
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled("Price (₹)", Style::default().fg(NEO_MUTED)))
                .style(Style::default().fg(NEO_BORDER))
                .bounds(y_bounds)
                .labels(y_labels),
        );

    frame.render_widget(chart, chunks[1]);

    // 3. Order Book Depth & Pressure Gauge
    let total_depth = (data.total_buy_qty + data.total_sell_qty) as f64;
    let buy_pct = if total_depth > 0.0 {
        (data.total_buy_qty as f64 / total_depth) * 100.0
    } else {
        50.0
    };
    let sell_pct = 100.0 - buy_pct;

    let bar_width: usize = 30;
    let buy_chars = ((buy_pct / 100.0) * (bar_width as f64)).round() as usize;
    let sell_chars = bar_width.saturating_sub(buy_chars);

    let depth_line = Line::from(vec![
        Span::raw("Order Book Pressure: ["),
        Span::styled("█".repeat(buy_chars), Style::default().fg(NEO_GREEN)),
        Span::styled("░".repeat(sell_chars), Style::default().fg(NEO_RED)),
        Span::raw("]  "),
        Span::styled(format!("BIDS: {:.0}% ({})", buy_pct, data.total_buy_qty), Style::default().fg(NEO_GREEN).add_modifier(Modifier::BOLD)),
        Span::raw("  vs  "),
        Span::styled(format!("ASKS: {:.0}% ({})", sell_pct, data.total_sell_qty), Style::default().fg(NEO_RED).add_modifier(Modifier::BOLD)),
        Span::raw("  │  Best Bid: "),
        Span::styled(format!("₹{:.2}", data.best_bid), Style::default().fg(NEO_GREEN)),
        Span::raw("  │  Best Ask: "),
        Span::styled(format!("₹{:.2}", data.best_ask), Style::default().fg(NEO_RED)),
    ]);

    let depth_block = Paragraph::new(depth_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(NEO_BORDER))
            .title(" Level 2 Order Flow & Microstructure Imbalance "),
    );
    frame.render_widget(depth_block, chunks[2]);
}
