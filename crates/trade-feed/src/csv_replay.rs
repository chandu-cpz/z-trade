use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use chrono::{DateTime, Local};
use trade_core::tick::Tick;

/// High-throughput zero-allocation CSV tick replay feed.
pub struct CsvReplayFeed {
    reader: BufReader<File>,
    line_buf: String,
}

impl CsvReplayFeed {
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(&path)?;
        let mut reader = BufReader::with_capacity(1024 * 1024, file);
        let mut line_buf = String::new();

        // Consume header if present
        if reader.read_line(&mut line_buf)? > 0 {
            if !line_buf.starts_with("timestamp") {
                // Not a header, seek back or recreate reader
                drop(reader);
                let f = File::open(&path)?;
                reader = BufReader::with_capacity(1024 * 1024, f);
            }
        }
        line_buf.clear();

        Ok(Self { reader, line_buf })
    }
}

impl Iterator for CsvReplayFeed {
    type Item = Tick;

    fn next(&mut self) -> Option<Self::Item> {
        self.line_buf.clear();
        while self.reader.read_line(&mut self.line_buf).ok()? > 0 {
            let line = self.line_buf.trim();
            if line.is_empty() || line.starts_with("timestamp") {
                self.line_buf.clear();
                continue;
            }

            let mut parts = line.split(',');
            let ts_str = parts.next()?;
            let symbol = parts.next()?;
            let price = parts.next()?.parse::<f64>().ok()?;
            let volume = parts.next()?.parse::<u64>().unwrap_or(0);
            let tbq = parts.next()?.parse::<u64>().unwrap_or(0);
            let tsq = parts.next()?.parse::<u64>().unwrap_or(0);
            let bp = parts.next()?.parse::<f64>().unwrap_or(0.0);
            let sp = parts.next()?.parse::<f64>().unwrap_or(0.0);
            let ltq = parts.next()?.parse::<u64>().unwrap_or(0);

            let timestamp = DateTime::parse_from_rfc3339(ts_str)
                .map(|dt| dt.with_timezone(&Local))
                .unwrap_or_else(|_| Local::now());

            return Some(Tick {
                symbol: symbol.to_string(),
                token: 0,
                price,
                volume,
                total_buy_qty: tbq,
                total_sell_qty: tsq,
                best_bid: bp,
                best_ask: sp,
                last_trade_qty: ltq,
                timestamp,
            });
        }
        None
    }
}
