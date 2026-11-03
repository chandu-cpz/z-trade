use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Instant;

use colored::Colorize;
use trade_core::config::Config;
use trade_feed::CsvReplayFeed;
use trade_sim::PaperSimulationEngine;
use trade_strategy::StrategyEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let csv_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "data/ticks_2026-02-19.csv".to_string()
    };

    if !Path::new(&csv_path).exists() {
        eprintln!(
            "{} File not found: {}",
            "ERROR:".bold().red(),
            csv_path.bold()
        );
        std::process::exit(1);
    }

    let config = Config::load_or_default("configs/default.toml");

    println!("{}", "═══════════════════════════════════════════════════════════════".cyan().bold());
    println!("{}", "             Z-TRADE HISTORICAL BACKTEST ENGINE                ".cyan().bold());
    println!("{}", "═══════════════════════════════════════════════════════════════".cyan().bold());
    println!("  Dataset:        {}", csv_path.yellow());
    println!("  Initial Margin: ₹{:.2}", config.portfolio.initial_capital);
    println!("  Max Positions:  {}", config.portfolio.max_positions);
    println!("  Max Risk Loss:  ₹{:.2}", config.portfolio.max_loss);
    println!("  Slippage Model: {:.2}%", config.transaction_costs.simulated_slippage_pct);
    println!("  Fees Model:     ₹{:.0}/order + {:.3}% STT + GST",
        config.transaction_costs.simulated_brokerage_per_order,
        config.transaction_costs.simulated_stt_pct
    );
    println!("{}", "───────────────────────────────────────────────────────────────");
    println!("Replaying market ticks...");

    let start_time = Instant::now();
    let mut feed = CsvReplayFeed::open(&csv_path)?;
    let mut sim_engine = PaperSimulationEngine::new(config.clone());
    let mut strat_engine = StrategyEngine::new(config.clone());

    let mut tick_count: u64 = 0;

    for tick in feed.by_ref() {
        tick_count += 1;

        // 1. Process exit / stop-loss / trailing stops on existing positions
        sim_engine.update_tick(&tick);

        // 2. Evaluate strategy rules on incoming tick
        if let Some(signal) = strat_engine.on_tick(&tick) {
            sim_engine.handle_signal(signal);
        }
    }

    // Close any remaining positions at end of session
    let open_keys: Vec<String> = sim_engine.positions.keys().cloned().collect();
    for key in open_keys {
        if let Some(pos) = sim_engine.positions.remove(&key) {
            let exit_price = pos.current_price;
            sim_engine.close_trade(pos, exit_price, "EndOfBacktest");
        }
    }

    let elapsed = start_time.elapsed();
    let ticks_per_sec = if elapsed.as_secs_f64() > 0.0 {
        tick_count as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!("\n{}", "═══════════════════════════════════════════════════════════════".cyan().bold());
    println!("{}", "                     BACKTEST RESULTS                          ".green().bold());
    println!("{}", "═══════════════════════════════════════════════════════════════".cyan().bold());
    println!("  Ticks Replayed:    {} in {:.2?} ({:.0} ticks/sec)",
        tick_count.to_string().yellow().bold(),
        elapsed,
        ticks_per_sec
    );

    let stats = &sim_engine.stats;
    let net_pnl = stats.total_net_pnl;
    let pnl_str = format!("₹{:.2}", net_pnl);
    let colored_pnl = if net_pnl >= 0.0 {
        pnl_str.green().bold()
    } else {
        pnl_str.red().bold()
    };

    println!("  Net Realized P&L:  {}", colored_pnl);
    println!("  Gross P&L:         ₹{:.2}", stats.total_gross_pnl);
    println!("  Total Fees & STT:  ₹{:.2}", stats.total_fees);
    println!("  Total Trades:      {}", stats.trades_count);
    println!("  Winning Trades:    {} ({:.1}%)", stats.winning_trades, stats.win_rate());
    println!("  Losing Trades:     {}", stats.losing_trades);
    println!("  Largest Win:       ₹{:.2}", stats.largest_win);
    println!("  Largest Loss:      ₹{:.2}", stats.largest_loss);

    // Strategy Breakdown
    let mut strat_stats: HashMap<String, (u32, u32, f64, f64)> = HashMap::new();
    for trade in &sim_engine.trades {
        let key = trade.strategy.to_string();
        let entry = strat_stats.entry(key).or_insert((0, 0, 0.0, 0.0));
        entry.0 += 1;
        if trade.net_pnl > 0.0 {
            entry.1 += 1;
        }
        entry.2 += trade.net_pnl;
        entry.3 += trade.transaction_costs;
    }

    println!("\n{}", "──────────────── STRATEGY PERFORMANCE BREAKDOWN ────────────────".cyan());
    println!("{:<16} {:>8} {:>10} {:>12} {:>12}", "Strategy", "Trades", "Win Rate", "Net P&L", "Fees Paid");
    println!("{}", "─".repeat(63));

    let mut sorted_strats: Vec<_> = strat_stats.iter().collect();
    sorted_strats.sort_by(|a, b| b.1 .2.partial_cmp(&a.1 .2).unwrap_or(std::cmp::Ordering::Equal));

    for (strat, (trades, wins, pnl, fees)) in sorted_strats {
        let wr = if *trades > 0 { (*wins as f64 / *trades as f64) * 100.0 } else { 0.0 };
        let pnl_c = if *pnl >= 0.0 {
            format!("₹{:.2}", pnl).green()
        } else {
            format!("₹{:.2}", pnl).red()
        };
        println!("{:<16} {:>8} {:>9.1}% {:>12} {:>12.2}", strat, trades, wr, pnl_c, fees);
    }

    if !sim_engine.trades.is_empty() {
        println!("\n{}", "──────────────── RECENT EXECUTED TRADES (LAST 10) ──────────────".cyan());
        println!("{:<9} {:<12} {:<5} {:>8} {:>8} {:>10} {:<15}", "Time", "Symbol", "Side", "Entry", "Exit", "Net P&L", "Reason");
        println!("{}", "─".repeat(72));
        for trade in sim_engine.trades.iter().rev().take(10).rev() {
            let pnl_c = if trade.net_pnl >= 0.0 {
                format!("₹{:.2}", trade.net_pnl).green()
            } else {
                format!("₹{:.2}", trade.net_pnl).red()
            };
            println!(
                "{:<9} {:<12} {:<5} {:>8.2} {:>8.2} {:>10} {:<15}",
                trade.exit_time.format("%H:%M:%S"),
                trade.symbol,
                trade.side,
                trade.entry_price,
                trade.exit_price,
                pnl_c,
                trade.exit_reason
            );
        }
    }

    println!("{}\n", "═══════════════════════════════════════════════════════════════".cyan().bold());
    Ok(())
}
