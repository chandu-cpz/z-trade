use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use trade_core::store::PendingOrder;
use trade_core::types::PositionSide;
use trade_sim::PaperSimulationEngine;

#[derive(Debug, Clone)]
pub struct OrderModalState {
    pub is_open: bool,
    pub symbol: String,
    pub side: PositionSide,
    pub order_type: String, // "MARKET", "LIMIT", "STOP_LOSS"
    pub quantity: u32,
    pub price: f64,
    pub limit_price: f64,
    pub stop_loss_pct: f64,
    pub target_pct: f64,
    pub active_field: usize, // 0: Type, 1: Qty, 2: Limit, 3: SL, 4: Target
}

impl Default for OrderModalState {
    fn default() -> Self {
        Self {
            is_open: false,
            symbol: "HDFCBANK-EQ".to_string(),
            side: PositionSide::Long,
            order_type: "MARKET".to_string(),
            quantity: 10,
            price: 100.0,
            limit_price: 100.0,
            stop_loss_pct: 0.5,
            target_pct: 1.0,
            active_field: 0,
        }
    }
}

impl OrderModalState {
    pub fn open(&mut self, symbol: String, side: PositionSide, current_price: f64) {
        self.is_open = true;
        self.symbol = symbol;
        self.side = side;
        self.price = if current_price > 0.0 { current_price } else { 100.0 };
        self.limit_price = self.price;
        self.active_field = 0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn to_pending_order(&self) -> PendingOrder {
        let sl_price = match self.side {
            PositionSide::Long => self.price * (1.0 - (self.stop_loss_pct / 100.0)),
            PositionSide::Short => self.price * (1.0 + (self.stop_loss_pct / 100.0)),
        };
        let target_price = match self.side {
            PositionSide::Long => self.price * (1.0 + (self.target_pct / 100.0)),
            PositionSide::Short => self.price * (1.0 - (self.target_pct / 100.0)),
        };

        PendingOrder {
            id: format!("MAN-{}", chrono::Local::now().format("%H%M%S%3f")),
            symbol: self.symbol.clone(),
            token: 0,
            side: self.side,
            order_type: self.order_type.clone(),
            quantity: self.quantity,
            limit_price: if self.order_type == "LIMIT" { Some(self.limit_price) } else { None },
            trigger_price: if self.order_type == "STOP_LOSS" { Some(self.limit_price) } else { None },
            stop_loss: Some(sl_price),
            target: Some(target_price),
            placed_at: chrono::Local::now(),
            origin: "MANUAL".to_string(),
        }
    }
}

pub fn render_order_modal(
    frame: &mut Frame,
    modal: &OrderModalState,
    engine: &PaperSimulationEngine,
    area: Rect,
) {
    if !modal.is_open {
        return;
    }

    let popup_area = centered_rect(55, 55, area);
    frame.render_widget(Clear, popup_area);

    let side_color = if modal.side == PositionSide::Long { Color::Rgb(0, 210, 144) } else { Color::Rgb(235, 87, 87) };
    let action_str = if modal.side == PositionSide::Long { "BUY / GO LONG" } else { "SELL / SHORT" };

    let total_cost = modal.price * (modal.quantity as f64);
    let free_cash = engine.free_capital();
    let margin_ok = free_cash >= total_cost;

    let content = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", action_str), Style::default().fg(Color::Black).bg(side_color).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(format!(" {} ", modal.symbol), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw(" @ LTP: "),
            Span::styled(format!("₹{:.2}", modal.price), Style::default().fg(Color::Yellow)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  [1] Order Type:   "),
            Span::styled(format!("[ {} ]", modal.order_type), Style::default().fg(if modal.active_field == 0 { Color::Yellow } else { Color::Cyan }).add_modifier(Modifier::BOLD)),
            Span::raw("  (Press 't' to toggle Market/Limit)"),
        ]),
        Line::from(vec![
            Span::raw("  [2] Quantity:     "),
            Span::styled(format!("{}", modal.quantity), Style::default().fg(if modal.active_field == 1 { Color::Yellow } else { Color::White }).add_modifier(Modifier::BOLD)),
            Span::raw(" shares  (Use '←'/ '→' or '+10' / '-10')"),
        ]),
        Line::from(vec![
            Span::raw("  [3] Limit Price:  "),
            Span::styled(format!("₹{:.2}", modal.limit_price), Style::default().fg(if modal.active_field == 2 { Color::Yellow } else { Color::White })),
        ]),
        Line::from(vec![
            Span::raw("  [4] Stop Loss:    "),
            Span::styled(format!("{:.2}% (₹{:.2})", modal.stop_loss_pct, modal.price * (modal.stop_loss_pct / 100.0)), Style::default().fg(Color::Rgb(235, 87, 87))),
        ]),
        Line::from(vec![
            Span::raw("  [5] Profit Target:"),
            Span::styled(format!("{:.2}% (₹{:.2})", modal.target_pct, modal.price * (modal.target_pct / 100.0)), Style::default().fg(Color::Rgb(0, 210, 144))),
        ]),
        Line::raw(""),
        Line::from(Span::styled("─── MARGIN & SIZING ────────────────────────", Style::default().fg(Color::Rgb(70, 85, 110)))),
        Line::from(vec![
            Span::raw("  Required Margin: "),
            Span::styled(format!("₹{:.2}", total_cost), Style::default().fg(if margin_ok { Color::White } else { Color::Red }).add_modifier(Modifier::BOLD)),
            Span::raw("  │  Available Cash: "),
            Span::styled(format!("₹{:.2}", free_cash), Style::default().fg(Color::Rgb(0, 210, 144))),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  [Enter] Confirm & Submit Order  ", Style::default().fg(Color::Black).bg(side_color).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("  [Esc] Cancel  ", Style::default().fg(Color::White).bg(Color::Rgb(51, 65, 85))),
        ]),
    ];

    let block = Block::default()
        .title(format!(" Manual Order Placement: {} ", modal.symbol))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(side_color));

    let p = Paragraph::new(content).block(block).alignment(Alignment::Left);
    frame.render_widget(p, popup_area);
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
