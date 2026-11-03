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

pub const KITE_BUY_BLUE: Color = Color::Rgb(56, 126, 209);
pub const KITE_SELL_ORANGE: Color = Color::Rgb(255, 87, 34);
pub const KITE_GREEN: Color = Color::Rgb(0, 210, 144);
pub const KITE_RED: Color = Color::Rgb(235, 87, 87);
pub const KITE_BORDER: Color = Color::Rgb(71, 85, 105);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductType {
    Mis, // Intraday with 5x leverage
    Cnc, // Delivery cash
}

#[derive(Debug, Clone)]
pub struct OrderModalState {
    pub is_open: bool,
    pub symbol: String,
    pub side: PositionSide,
    pub product: ProductType,
    pub order_type: String, // "MARKET", "LIMIT", "SL"
    pub quantity: u32,
    pub price: f64,
    pub limit_price: f64,
    pub trigger_price: f64,
    pub active_field: usize, // 0: Product, 1: Type, 2: Qty, 3: Price
}

impl Default for OrderModalState {
    fn default() -> Self {
        Self {
            is_open: false,
            symbol: "RELIANCE".to_string(),
            side: PositionSide::Long,
            product: ProductType::Mis,
            order_type: "MARKET".to_string(),
            quantity: 50,
            price: 2940.50,
            limit_price: 2940.50,
            trigger_price: 0.0,
            active_field: 2, // focus quantity by default
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
        self.order_type = "MARKET".to_string();
        self.active_field = 2; // Focus Qty
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn toggle_product(&mut self) {
        self.product = match self.product {
            ProductType::Mis => ProductType::Cnc,
            ProductType::Cnc => ProductType::Mis,
        };
    }

    pub fn toggle_order_type(&mut self) {
        self.order_type = match self.order_type.as_str() {
            "MARKET" => "LIMIT".to_string(),
            "LIMIT" => "SL".to_string(),
            _ => "MARKET".to_string(),
        };
    }

    pub fn required_margin(&self) -> f64 {
        let exec_price = if self.order_type == "LIMIT" {
            self.limit_price
        } else {
            self.price
        };
        let gross = exec_price * (self.quantity as f64);
        match self.product {
            ProductType::Mis => gross / 5.0, // 5x leverage for MIS
            ProductType::Cnc => gross,       // 1x full cash for Delivery
        }
    }

    pub fn to_pending_order(&self) -> PendingOrder {
        let _exec_price = if self.order_type == "LIMIT" {
            self.limit_price
        } else {
            self.price
        };

        let prod_str = match self.product {
            ProductType::Mis => "MIS",
            ProductType::Cnc => "CNC",
        };

        PendingOrder {
            id: format!("MAN-{}", chrono::Local::now().format("%H%M%S%3f")),
            symbol: self.symbol.clone(),
            token: 0,
            side: self.side,
            order_type: self.order_type.clone(),
            quantity: self.quantity,
            limit_price: if self.order_type == "LIMIT" { Some(self.limit_price) } else { None },
            trigger_price: if self.order_type == "SL" { Some(self.trigger_price) } else { None },
            stop_loss: None,
            target: None,
            placed_at: chrono::Local::now(),
            origin: format!("KITE_{}", prod_str),
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

    let popup_area = centered_rect(58, 48, area);
    frame.render_widget(Clear, popup_area);

    let is_buy = modal.side == PositionSide::Long;
    let header_color = if is_buy { KITE_BUY_BLUE } else { KITE_SELL_ORANGE };
    let action_title = if is_buy { "BUY" } else { "SELL" };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header banner (Blue for Buy, Orange for Sell)
            Constraint::Length(3), // Product Selector (MIS vs CNC)
            Constraint::Length(3), // Order Type Selector (MARKET vs LIMIT vs SL)
            Constraint::Length(4), // Qty & Price inputs
            Constraint::Length(4), // Margin info card
            Constraint::Length(3), // Action buttons
        ])
        .split(popup_area);

    // 1. Header Banner
    let header_text = format!(
        " {} {}  x  NSE   │   LTP: ₹{:.2} ",
        action_title, modal.symbol, modal.price
    );
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            header_text,
            Style::default().fg(Color::White).bg(header_color).add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(header_color)),
    );
    frame.render_widget(header, chunks[0]);

    // 2. Product Selector (MIS vs CNC)
    let mis_style = if modal.product == ProductType::Mis {
        Style::default().fg(Color::Black).bg(header_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let cnc_style = if modal.product == ProductType::Cnc {
        Style::default().fg(Color::Black).bg(header_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let product_p = Paragraph::new(Line::from(vec![
        Span::raw(" Product:   "),
        Span::styled(" [● Intraday (MIS)] ", mis_style),
        Span::raw("    "),
        Span::styled(" [○ Longterm (CNC)] ", cnc_style),
        Span::styled("  (Press 'm' or 'c' to toggle)", Style::default().fg(Color::DarkGray)),
    ]))
    .block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(KITE_BORDER)),
    );
    frame.render_widget(product_p, chunks[1]);

    // 3. Order Type Selector
    let mkt_style = if modal.order_type == "MARKET" {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let lmt_style = if modal.order_type == "LIMIT" {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let sl_style = if modal.order_type == "SL" {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let type_p = Paragraph::new(Line::from(vec![
        Span::raw(" Type:      "),
        Span::styled(" [MARKET] ", mkt_style),
        Span::raw(" "),
        Span::styled(" [LIMIT] ", lmt_style),
        Span::raw(" "),
        Span::styled(" [SL] ", sl_style),
        Span::styled("  (Press 't' to toggle)", Style::default().fg(Color::DarkGray)),
    ]))
    .block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(KITE_BORDER)),
    );
    frame.render_widget(type_p, chunks[2]);

    // 4. Inputs: Qty & Price
    let qty_focus = if modal.active_field == 2 {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::White)
    };
    let price_focus = if modal.active_field == 3 {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::White)
    };

    let price_str = if modal.order_type == "MARKET" {
        "AT MARKET".to_string()
    } else {
        format!("₹{:.2}", modal.limit_price)
    };

    let inputs_line = Line::from(vec![
        Span::raw(" Qty: "),
        Span::styled(format!(" [ {} ] ", modal.quantity), qty_focus),
        Span::styled(" (◄/► to adjust) ", Style::default().fg(Color::DarkGray)),
        Span::raw("  Price: "),
        Span::styled(format!(" [ {} ] ", price_str), price_focus),
        Span::styled(" (▲/▼ to adjust)", Style::default().fg(Color::DarkGray)),
    ]);
    let inputs_p = Paragraph::new(inputs_line).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(KITE_BORDER)),
    );
    frame.render_widget(inputs_p, chunks[3]);

    // 5. Margin Info Card
    let req_margin = modal.required_margin();
    let avail_margin = engine.free_capital();
    let has_margin = avail_margin >= req_margin;
    let margin_status_span = if has_margin {
        Span::styled(" ● SUFFICIENT MARGIN ", Style::default().fg(KITE_GREEN).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" ✕ INSUFFICIENT FUNDS ", Style::default().fg(KITE_RED).add_modifier(Modifier::BOLD))
    };

    let margin_lines = vec![
        Line::from(vec![
            Span::raw(" Margin Required: "),
            Span::styled(format!("₹{:.2}", req_margin), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw("   │   Available: "),
            Span::styled(format!("₹{:.2}", avail_margin), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw(" Order Status:   "),
            margin_status_span,
            Span::styled(
                if modal.product == ProductType::Mis { " (5x Intraday Leverage)" } else { " (1x Delivery Cash)" },
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];
    let margin_p = Paragraph::new(margin_lines).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::TOP)
            .border_style(Style::default().fg(KITE_BORDER)),
    );
    frame.render_widget(margin_p, chunks[4]);

    // 6. Action Footer
    let actions = Line::from(vec![
        Span::styled(
            " [ ENTER ] SUBMIT ORDER ",
            Style::default().fg(Color::Black).bg(header_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
        Span::styled(
            " [ ESC ] CANCEL ",
            Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 60)),
        ),
    ]);
    let actions_p = Paragraph::new(actions)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(header_color)),
        );
    frame.render_widget(actions_p, chunks[5]);
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
