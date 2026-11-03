use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Line as CLine, Rectangle as CRect},
        Block, BorderType, Borders, Paragraph,
    },
    Frame,
};
use trade_strategy::ChartData;

pub const KITE_GREEN: Color = Color::Rgb(0, 210, 144);
pub const KITE_RED: Color = Color::Rgb(235, 87, 87);
pub const KITE_BLUE: Color = Color::Rgb(56, 126, 209);
pub const KITE_MUTED: Color = Color::Rgb(148, 163, 184);
pub const KITE_BORDER: Color = Color::Rgb(51, 65, 85);
pub const KITE_BG_DARK: Color = Color::Rgb(15, 23, 42);

pub fn render_stock_view(
    frame: &mut Frame,
    chart_data: Option<&ChartData>,
    show_depth: bool,
    timeframe_label: &str,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Stock Header & OHLC Bar
            Constraint::Min(10),   // Chart & Market Depth Split
        ])
        .split(area);

    let Some(data) = chart_data else {
        let p = Paragraph::new("Select a stock from the Watchlist (Left) using [j]/[k] or [/] to search.")
            .style(Style::default().fg(KITE_MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(KITE_BORDER))
                    .title(" No Stock Selected "),
            );
        frame.render_widget(p, area);
        return;
    };

    // 1. Stock Header with Live Quote & Stats
    render_stock_header(frame, data, timeframe_label, chunks[0]);

    // 2. Chart & Depth Split
    if show_depth && chunks[1].width > 80 {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(68), // Candlestick Chart (68%)
                Constraint::Percentage(32), // 5-Depth Market Ladder (32%)
            ])
            .split(chunks[1]);

        render_candlestick_chart(frame, data, h_chunks[0]);
        render_5_depth(frame, data, h_chunks[1]);
    } else {
        render_candlestick_chart(frame, data, chunks[1]);
    }
}

fn render_stock_header(frame: &mut Frame, data: &ChartData, timeframe_label: &str, area: Rect) {
    let first_price = data.candles.first().map(|c| c.open).unwrap_or(data.current_price);
    let chg_val = data.current_price - first_price;
    let chg_pct = if first_price > 0.0 {
        (chg_val / first_price) * 100.0
    } else {
        0.0
    };

    let is_up = chg_val >= 0.0;
    let chg_color = if is_up { KITE_GREEN } else { KITE_RED };
    let arrow = if is_up { "▲ +" } else { "▼ " };

    let (open_p, high_p, low_p, vol_val) = if let Some(last) = data.candles.last() {
        (last.open, last.high, last.low, last.volume)
    } else {
        (data.current_price, data.max_price, data.min_price, 0)
    };

    let rsi_color = if data.rsi_val > 70.0 {
        KITE_RED
    } else if data.rsi_val < 30.0 {
        KITE_GREEN
    } else {
        Color::Cyan
    };

    let header_line = Line::from(vec![
        Span::styled(format!(" {} ", data.symbol), Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" NSE  │  "),
        Span::styled(format!("₹{:.2}", data.current_price), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(format!("{}{:.2} ({:.2}%)", arrow, chg_val.abs(), chg_pct), Style::default().fg(chg_color).add_modifier(Modifier::BOLD)),
        Span::raw("  │  "),
        Span::styled(format!("O: {:.2}", open_p), Style::default().fg(Color::Gray)),
        Span::raw("  "),
        Span::styled(format!("H: {:.2}", high_p), Style::default().fg(KITE_GREEN)),
        Span::raw("  "),
        Span::styled(format!("L: {:.2}", low_p), Style::default().fg(KITE_RED)),
        Span::raw("  "),
        Span::styled(format!("Vol: {}", format_volume(vol_val)), Style::default().fg(Color::Yellow)),
        Span::raw("  │  VWAP: "),
        Span::styled(format!("₹{:.2}", data.vwap_val), Style::default().fg(Color::Yellow)),
        Span::raw("  EMA: "),
        Span::styled(format!("₹{:.2}", data.ema_val), Style::default().fg(Color::Magenta)),
        Span::raw("  RSI: "),
        Span::styled(format!("{:.1}", data.rsi_val), Style::default().fg(rsi_color)),
        Span::raw("  │  TF: "),
        Span::styled(format!("[{}]", timeframe_label), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(KITE_BORDER));
    frame.render_widget(Paragraph::new(header_line).block(block), area);
}

fn render_candlestick_chart(frame: &mut Frame, data: &ChartData, area: Rect) {
    let candle_count = data.candles.len();
    let num_bars = candle_count.max(20);

    let mut min_p = data.min_price;
    let mut max_p = data.max_price;
    if (max_p - min_p).abs() < 0.05 {
        min_p -= 1.0;
        max_p += 1.0;
    }
    // Add 2% padding on y-axis
    let y_padding = (max_p - min_p) * 0.04;
    min_p = (min_p - y_padding).max(0.0);
    max_p += y_padding;

    let x_bounds = [0.0, num_bars as f64 * 3.0 + 2.0];
    let y_bounds = [min_p, max_p];

    let candles_ref = &data.candles;
    let vwap_val = data.vwap_val;
    let ema_val = data.ema_val;
    let curr_price = data.current_price;

    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(KITE_BLUE))
                .title(format!(" {} Candlestick Chart (OHLC) [1: 1m  2: 5m  3: 15m] ", data.symbol)),
        )
        .marker(symbols::Marker::HalfBlock)
        .x_bounds(x_bounds)
        .y_bounds(y_bounds)
        .paint(move |ctx| {
            // Draw VWAP reference line
            if vwap_val > 0.0 {
                ctx.draw(&CLine {
                    x1: x_bounds[0],
                    y1: vwap_val,
                    x2: x_bounds[1],
                    y2: vwap_val,
                    color: Color::Yellow,
                });
            }

            // Draw EMA reference line
            if ema_val > 0.0 {
                ctx.draw(&CLine {
                    x1: x_bounds[0],
                    y1: ema_val,
                    x2: x_bounds[1],
                    y2: ema_val,
                    color: Color::Magenta,
                });
            }

            // Draw current price line
            if curr_price > 0.0 {
                ctx.draw(&CLine {
                    x1: x_bounds[0],
                    y1: curr_price,
                    x2: x_bounds[1],
                    y2: curr_price,
                    color: Color::DarkGray,
                });
            }

            // Draw each candle
            for (i, candle) in candles_ref.iter().enumerate() {
                let x = i as f64 * 3.0;
                let is_bull = candle.close >= candle.open;
                let color = if is_bull { KITE_GREEN } else { KITE_RED };

                // Wick (high to low)
                ctx.draw(&CLine {
                    x1: x + 1.0,
                    y1: candle.low,
                    x2: x + 1.0,
                    y2: candle.high,
                    color,
                });

                // Candle body
                let (body_bottom, body_top) = if is_bull {
                    (candle.open, candle.close)
                } else {
                    (candle.close, candle.open)
                };
                let height = (body_top - body_bottom).max((max_p - min_p) * 0.008);

                ctx.draw(&CRect {
                    x,
                    y: body_bottom,
                    width: 2.0,
                    height,
                    color,
                });
            }
        });

    frame.render_widget(canvas, area);
}

fn render_5_depth(frame: &mut Frame, data: &ChartData, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // 5-Depth Ladder table
            Constraint::Length(3), // Buy/Sell Ratio Progress Bar
            Constraint::Min(2),    // Circuit Limits & Spread
        ])
        .split(area);

    // 5 depth rows
    let spread = 0.05;
    let b1 = (data.best_bid * 100.0).round() / 100.0;
    let a1 = (data.best_ask * 100.0).round() / 100.0;

    let bids = [
        (4, ((data.total_buy_qty / 5).max(10)), b1),
        (2, ((data.total_buy_qty / 7).max(10)), (b1 - spread)),
        (8, ((data.total_buy_qty / 4).max(10)), (b1 - spread * 2.0)),
        (1, ((data.total_buy_qty / 9).max(10)), (b1 - spread * 3.0)),
        (12, ((data.total_buy_qty / 6).max(10)), (b1 - spread * 4.0)),
    ];

    let asks = [
        (a1, ((data.total_sell_qty / 5).max(10)), 3),
        (a1 + spread, ((data.total_sell_qty / 7).max(10)), 2),
        (a1 + spread * 2.0, ((data.total_sell_qty / 4).max(10)), 6),
        (a1 + spread * 3.0, ((data.total_sell_qty / 9).max(10)), 1),
        (a1 + spread * 4.0, ((data.total_sell_qty / 6).max(10)), 9),
    ];

    let mut depth_lines = vec![Line::from(vec![
        Span::styled(" QTY    BID ", Style::default().fg(KITE_GREEN).add_modifier(Modifier::BOLD)),
        Span::raw(" │ "),
        Span::styled(" ASK    QTY ", Style::default().fg(KITE_RED).add_modifier(Modifier::BOLD)),
    ])];

    for i in 0..5 {
        let (_b_orders, b_qty, b_price) = bids[i];
        let (a_price, a_qty, _a_orders) = asks[i];
        depth_lines.push(Line::from(vec![
            Span::styled(format!("{:>5} {:>7.2}", b_qty, b_price), Style::default().fg(KITE_GREEN)),
            Span::raw(" │ "),
            Span::styled(format!("{:<7.2} {:<5}", a_price, a_qty), Style::default().fg(KITE_RED)),
        ]));
    }

    let ladder = Paragraph::new(depth_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(KITE_BORDER))
            .title(" Market Depth (5-Depth) "),
    );
    frame.render_widget(ladder, chunks[0]);

    // Ratio Bar
    let total_depth = (data.total_buy_qty + data.total_sell_qty) as f64;
    let buy_pct = if total_depth > 0.0 {
        (data.total_buy_qty as f64 / total_depth) * 100.0
    } else {
        50.0
    };
    let sell_pct = 100.0 - buy_pct;

    let bar_width = 16.min(chunks[1].width.saturating_sub(12) as usize);
    let buy_chars = ((buy_pct / 100.0) * (bar_width as f64)).round() as usize;
    let sell_chars = bar_width.saturating_sub(buy_chars);

    let ratio_line = Line::from(vec![
        Span::styled(format!("{:.0}% ", buy_pct), Style::default().fg(KITE_GREEN).add_modifier(Modifier::BOLD)),
        Span::raw("["),
        Span::styled("█".repeat(buy_chars), Style::default().fg(KITE_GREEN)),
        Span::styled("░".repeat(sell_chars), Style::default().fg(KITE_RED)),
        Span::raw("] "),
        Span::styled(format!("{:.0}%", sell_pct), Style::default().fg(KITE_RED).add_modifier(Modifier::BOLD)),
    ]);
    let ratio_p = Paragraph::new(ratio_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(KITE_BORDER)),
    );
    frame.render_widget(ratio_p, chunks[1]);

    // Circuit limits
    let upper_circuit = data.current_price * 1.10;
    let lower_circuit = data.current_price * 0.90;
    let circuits = Line::from(vec![
        Span::raw(" UC: "),
        Span::styled(format!("₹{:.2}", upper_circuit), Style::default().fg(KITE_GREEN)),
        Span::raw("  LC: "),
        Span::styled(format!("₹{:.2}", lower_circuit), Style::default().fg(KITE_RED)),
    ]);
    frame.render_widget(Paragraph::new(circuits), chunks[2]);
}

fn format_volume(vol: u64) -> String {
    if vol >= 10_000_000 {
        format!("{:.2}Cr", vol as f64 / 10_000_000.0)
    } else if vol >= 100_000 {
        format!("{:.2}L", vol as f64 / 100_000.0)
    } else if vol >= 1_000 {
        format!("{:.1}k", vol as f64 / 1_000.0)
    } else {
        vol.to_string()
    }
}
