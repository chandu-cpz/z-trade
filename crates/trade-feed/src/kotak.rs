use std::collections::HashMap;
use chrono::Local;
use kotak_neo_api::api::quote::ExchangeSegment;
use kotak_neo_api::auth::KotakNeo;
use kotak_neo_api::websockets::market_feed::{FeedType, MarketEvent, MarketFeedHandle, Scrip};
use kotak_neo_api::{ScripInfo, ScripListReader};
use trade_core::tick::Tick;

pub struct KotakFeed {
    handle: MarketFeedHandle,
    token_to_symbol: HashMap<String, String>,
}

impl KotakFeed {
    pub async fn connect(
        api: &KotakNeo,
        max_symbols: Option<usize>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let stocks: Vec<ScripInfo> = ScripListReader::get_equity_stocks(ExchangeSegment::NseCm)?;
        let mut token_to_symbol = HashMap::new();
        let mut scrips = Vec::new();

        let filtered = stocks.iter().filter(|s| s.is_equity());
        let count = max_symbols.unwrap_or(2413);

        for s in filtered.take(count) {
            token_to_symbol.insert(s.token.clone(), s.trading_symbol.clone());
            scrips.push(Scrip::new(ExchangeSegment::NseCm, &s.token));
        }

        let handle = api.market_feed().await?;
        handle.subscribe(&scrips, FeedType::Scrip).await?;

        Ok(Self {
            handle,
            token_to_symbol,
        })
    }

    pub async fn next_tick(&mut self) -> Option<Tick> {
        while let Some(event) = self.handle.next().await {
            match event {
                MarketEvent::Tick(m_tick) => {
                    let ltp_str = m_tick.data.get("ltp")?;
                    let price = ltp_str.parse::<f64>().ok()?;
                    if price <= 0.0 {
                        continue;
                    }

                    let symbol = self
                        .token_to_symbol
                        .get(&m_tick.token)
                        .cloned()
                        .or(m_tick.trading_symbol)
                        .unwrap_or_else(|| m_tick.token.clone());

                    let volume = m_tick
                        .data
                        .get("v")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let tbq = m_tick
                        .data
                        .get("tbq")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let tsq = m_tick
                        .data
                        .get("tsq")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let bp = m_tick
                        .data
                        .get("bp")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let sp = m_tick
                        .data
                        .get("sp")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let ltq = m_tick
                        .data
                        .get("ltq")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let token = m_tick.token.parse::<u32>().unwrap_or(0);

                    return Some(Tick {
                        symbol,
                        token,
                        price,
                        volume,
                        total_buy_qty: tbq,
                        total_sell_qty: tsq,
                        best_bid: bp,
                        best_ask: sp,
                        last_trade_qty: ltq,
                        timestamp: Local::now(),
                    });
                }
                MarketEvent::Connected => {
                    log::info!("Kotak SFeed market stream connected");
                }
                MarketEvent::Disconnected(reason) => {
                    log::warn!("Kotak SFeed market stream disconnected: {:?}", reason);
                }
            }
        }
        None
    }
}
