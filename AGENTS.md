# Agent Guidelines & Operational Rules

## Core Trading Rule
> **CRITICAL**: **WE DO NOT DO REAL TRADING. ONLY PAPER TRADES ARE PERMITTED.**
> Under no circumstances should real money orders be placed via any broker API. All strategy executions, order placements, and simulations must strictly use simulated / paper trading accounts and mocked or paper execution engines.

## Workspace Architecture & Code Standards
1. **Paper Trading Only**:
   - Live orders must be disabled or stubbed out.
   - Any order routing logic must route exclusively to the local paper-trading simulator.
2. **Modular Workspace**:
   - Single clean Cargo workspace.
   - Common domain models, order books, technical indicators, and strategy abstractions are shared across crates rather than copy-pasted.
   - Clean separation of concerns:
     - `trade-core`: Domain models, enums, positions, orders, events.
     - `trade-indicators`: Technical indicators (VWAP, RSI, EMA, Order Book Imbalance, Slopes).
     - `trade-strategy`: Strategy abstraction (`Strategy` trait) and concrete strategies (Mean Reversion, Momentum, Liquidity Sweep, ORB).
     - `trade-sim`: Paper trading engine, fill simulation, PnL & risk accounting.
     - `trade-tui`: Unified Ratatui terminal dashboard.
3. **Safety & Secrets**:
   - Never commit or log API keys, TOTP secrets, user credentials, or `.env` contents.
