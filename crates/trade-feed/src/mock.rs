use std::time::Duration;
use chrono::Local;
use trade_core::tick::Tick;

#[derive(Clone, Debug)]
pub struct MockStock {
    pub symbol: String,
    pub base_price: f64,
    pub current_price: f64,
    pub open_price: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub volume: u64,
}

pub struct MockFeed {
    pub stocks: Vec<MockStock>,
    current_idx: usize,
    last_tick: std::time::Instant,
    interval: Duration,
    rng_state: u64,
}

impl Default for MockFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl MockFeed {
    pub fn new() -> Self {
        let initial_stocks = vec![
            ("RELIANCE", 2940.50),
            ("TCS", 3890.00),
            ("HDFCBANK", 1425.80),
            ("INFY", 1680.20),
            ("ICICIBANK", 1090.10),
            ("SBIN", 760.40),
            ("TATAMOTORS", 980.50),
            ("ITC", 412.30),
            ("BHARTIARTL", 1290.00),
            ("KOTAKBANK", 1780.00),
            ("LT", 3540.00),
            ("AXISBANK", 1150.00),
            ("MARUTI", 12400.00),
            ("SUNPHARMA", 1620.00),
            ("TITAN", 3450.00),
        ];

        let stocks = initial_stocks
            .into_iter()
            .map(|(sym, price)| MockStock {
                symbol: sym.to_string(),
                base_price: price,
                current_price: price,
                open_price: price,
                high_price: price * 1.002,
                low_price: price * 0.998,
                volume: 120_000,
            })
            .collect();

        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x853c49e6748fea9b);

        Self {
            stocks,
            current_idx: 0,
            last_tick: std::time::Instant::now(),
            interval: Duration::from_millis(40),
            rng_state: seed | 1,
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn poll_tick(&mut self) -> Option<Tick> {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick) < self.interval {
            return None;
        }
        self.last_tick = now;

        let num_stocks = self.stocks.len();
        if num_stocks == 0 {
            return None;
        }

        let idx = self.current_idx;
        self.current_idx = (self.current_idx + 1) % num_stocks;

        // Pre-generate random values before borrowing stock mutably
        let rand_f = self.next_f64();
        let rand_qty = (self.next_u64() % 200 + 5) as u64;
        let rand_tbq = (self.next_u64() % 50_000 + 20_000) as u64;
        let rand_tsq = (self.next_u64() % 50_000 + 20_000) as u64;

        let stock = &mut self.stocks[idx];

        // Random walk delta: -0.15% to +0.15%
        let delta_pct = (rand_f - 0.5) * 0.003;
        let tick_delta = stock.current_price * delta_pct;
        // Snap to NSE 0.05 tick size
        let new_price = ((stock.current_price + tick_delta) * 20.0).round() / 20.0;
        let new_price = new_price.max(stock.base_price * 0.8).min(stock.base_price * 1.2);

        stock.current_price = new_price;
        if new_price > stock.high_price {
            stock.high_price = new_price;
        }
        if new_price < stock.low_price {
            stock.low_price = new_price;
        }

        stock.volume += rand_qty;

        let spread = 0.05;
        let best_bid = (new_price - spread).max(0.05);
        let best_ask = new_price + spread;

        Some(Tick {
            symbol: stock.symbol.clone(),
            token: 0,
            price: new_price,
            volume: stock.volume,
            total_buy_qty: rand_tbq,
            total_sell_qty: rand_tsq,
            best_bid,
            best_ask,
            last_trade_qty: rand_qty,
            timestamp: Local::now(),
        })
    }
}
