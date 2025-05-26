# Superliquid: HotStuff-Based BFT Consensus with native spot orderbook DEX (Work-in-Progress)

A Rust implementation of a **Byzantine Fault Tolerant (BFT)** consensus protocol inspired by [HotStuff](https://arxiv.org/abs/1803.05069), extended with a native spot decentralised exchange (DEX) integrated directly into the execution layer. This project aims to demonstrate rotating leader consensus, peer-to-peer networking, chained hotstuff pipelining and native DEX orderbook matching. 

> **Status**: Basic Layer one blockchain and Spot DEX functionality are completed. Perps DEX in development. Any and all feedback is welcome! 

---

## Table of Contents

1. [Overview](#overview)  
2. [Key Features](#key-features)  
3. [Architecture](#architecture)
4. [Key Components](#key-components)
5. [Design Decisions](#design-decisions)
6. [Getting Started](#getting-started)  
7. [Usage](#usage)  
8. [Roadmap](#roadmap)  
9. [License](#license)  
10. [References](#references)

---

## Overview

This repository implements the core components of a HotStuff-style consensus mechanism, designed for high throughput and fault tolerance:

* **Pacemaker**: Manages view changes, including timeouts, new-view messages, and leader rotation.
* **Peer-to-Peer Network Layer**: Built with `tokio` for efficient asynchronous communication.
* **Replica Logic**: Implements the chained HotStuff pipelining for performance.
* **Leader Rotation**: Employs a round-robin strategy (`leader_id = view % num_replicas`).
* **Byzantine Fault Tolerance**: Designed to tolerate up to `$f$` Byzantine nodes in a network of `$3f + 1$` replicas, ensuring safety and liveness.
* **Integrated Spot DEX**: Features on-chain order book matching and settlement as part of the block execution, using **price-time** priority directly from the state.

The purpose of this project is to learn, experiment, and demonstrate a BFT consensus mechanism in Rust along with in-protocol financial primitives. The consensus engine and spot DEX are largely complete. Work is ongoing to extend support to a perpetuals DEX with margin, funding rates, and liquidations. This project is not production-ready.

---

## Key Features

- **Async Rust Networking**: Uses `tokio` to handle concurrent socket connections between replicas and clients.
- **Chained HotStuff**: Enables pipelined consensus with lower latency.
- **Native Spot DEX**: Matching engine executed within the consensus logic based on on-chain order book
- **Deterministic Leader Election**: A rotating leader ensures fairness and mitigates the risk of a single faulty leader stalling the protocol.
- **Exponential Timeouts**: If the protocol doesn’t make progress in a view, replicas increase their timeout before moving to the next view.
- **View Synchronization**: Fast-forward logic ensures that once any replica observes a higher-view message, it jumps to that view to maintain synchronization.
- **Eventual Safety and Liveness**: Adheres to the HotStuff design, ensuring that honest replicas eventually agree on a final sequence of blocks.
- **Priority-Based Mempool**: Mempool prioritizes critical transactions like liquidations and cancels.
---
## Architecture

The replica node architecture is modular, separating concerns for clarity and maintainability:

* **Consensus Layer**: Handles the core HotStuff protocol, including block proposals, voting, QC formation, commitment, and state management related to consensus.
* **Execution Layer**: Responsible for applying transactions, managing the application state (including the spot clearinghouse), and enforcing state transition rules.
* **Networking and Communication Layer**: Manages all peer-to-peer TCP connections for consensus messages and client interactions, with built-in reconnection logic.


System Data Flow:

```text
Client Transactions
    ↓
PriorityMempool (transaction staging and prioritization)
    ↓
Replica Logic (leader election, block proposal, voting)
    ↓
MessageWindow (view-based message caching)
    ↓
LedgerState (validated transaction application)
    ↓
Committed Blocks (state finalisation)
```


---

## Key components
### Replica
- Orchestrates block proposal and voting based on the current leader and view number.
- Interfaces with pacemaker, mempool, and ledger state.

### Pacemaker
- Drives view advancement based on timeouts.
- Triggers leader rotation and synchronizes view progression across replicas.

### PriorityMempool
- Manages incoming transactions, ordering them by account nonce.
- Prioritizes urgent operations (i.e. liquidations, cancels) over normal transfers.

### LedgerState
- Validates and applies transactions from committed blocks.
- Maintains the canonical state, including account balances and DEX order books.

### Spot Clearinghouse
- Maintains the in-protocol order books for each market.
- Maintains a live in-protocol orderbook per market.
- Applies limit and market orders based on price-time priority.
- Order execution occurs on commit, ensuring consistency across all honest replicas.

---

## Design Decisions

### Consensus Model
- Implements chained HotStuff for pipelined block progress with reduced view latency.
- Enforces safety (no conflicting commits) and liveness (eventual commit under partial synchrony) by following core HotStuff invariants.
- The consensus layer validates block structure and transaction admissibility, but does not execute transactions or validate state transitions. This design follows the HotStuff separation of consensus and execution, where the leader proposes a block containing valid transactions, and all replicas deterministically apply the same state transition logic upon commit.

### Pacemaker
- Uses exponential backoff timers to handle partial synchrony and avoid view lockstep issues.
- Fast-forwarding enables replicas to synchronize with minimal downtime.

### PriorityMempool
- Organizes pending transactions by per-account nonce, enforcing strict sequential execution, while allowing for replacement of inflight pending transaction if needed.
- Prioritizes urgent actions (e.g., liquidations, cancels) over normal transfers.
- Designed for future scalability: insertion is O(log n) per account, and per-account queue sizes are expected to remain small. 

Future improvements:
- Enforce a hard cap on pending transactions per account to mitigate spam attacks.
- Support bundling multiple sequential transactions per account into a single block to improve liquidation throughput.


### Spot Clearinghouse
- Maintains a protocol-level orderbook per market
- Applies limit and market orders based on price-time priority.
- Order execution occurs on commit, ensuring consistency across all honest replicas.

### Networking and State Management
- Built on `tokio` async primitives to support concurrent peer connections and message passing.
- Ledger state is updated only on committed blocks, ensuring consistency across replicas.
- Transactions are validated against the ledger state during block execution to ensure determinism.

---

## Getting Started

### Prerequisites

- **Rust** (1.65+ recommended)  
  Install Rust from [rustup](https://rustup.rs/) or your preferred package manager.  
- **Cargo** (bundled with Rust)  
  Ensure you can run `cargo --version`.

### Installation

1. **Clone the Repository**  
```bash
git clone https://github.com/shawnlimjunhe/superliquid.git  
cd superliquid  
```

2. **Build the Project**  
```bash
cargo build
```

3. **Run Tests** (If desired) 
```bash
cargo test
```

---

## Usage

### Running a Node

Each node is run via a positional `node <id>` argument in individual terminals:
As of now, the protocol requires 4 nodes to run and can tolerate up to 1 node failing.

```bash
cargo run -- node 0   // run node 0  
cargo run -- node 1   // run node 1  
cargo run -- node 2   // run node 2  
cargo run -- node 3   // run node 3  
```
The network may take a short time to stabilize after startup

### Running the Client Console
You can run a **client console** to interact with the network by starting a console instance:

```bash
cargo run -- console 0 // Connects to node 0
```

The client console provides commands to:

- Create/load accounts (Ed25519 keypairs)
- Request a drip (faucet funding) 
- Query your account balance
- View markets
- Place a limit / market order
- Cancel order
- Query open orders
  
After starting the console, type `help` to see available commands.

#### Example flow
1. Create a user account with the `create` command.
![Create account](./assets/create.png)
2. Drip `USD` or `SUPE` to your account using `drip SUPE` or `drip USD`.
![Drip funds into account](./assets/drip.png)
3. Query your account balance using the `query` command.
4. View available markets with the "markets" command.
5. Select the "SUPE/USD" market with id: 0. The "SUPE" and "USD"   
assets and "SUPE/USD" market are created on genesis.
![View and select markets](./assets/markets.png)
6. Submit a limit buy order
![Before Limit Buy](./assets/limit_buy.png)
7. The submitted order should be reflected on the market. (refresh if changes do not show immediately)
![Before Limit Buy](./assets/limit_buy_post.png)
8. Create and fund another account to fill this order
![Create account 2](./assets/create_account2.png)
9. Fill account 1's limit buy order with a market sell
![Market sell](./assets/market_sell.png)
10. Orders should match and settle with both parties' balances having been updated
- Account 1:
![Filled order](./assets/limit_fill.png)
- Account 2:
![Filled order](./assets/market_sell_post.png)




### Setup Keys with `.env`

To provide each node with its identity:
1. Rename the provided `.env-example` file to `.env`
2. This file contains **randomly generated private keys** for each node

Ensure each running instance has the appropriate `.env` file available in its working directory.

### Observing Logs

You can monitor the console output to see:

- **View changes** when the pacemaker times out or receives higher-view messages.

---

## Roadmap
**Perps DEX**
- [ ] Represent leveraged long/short positions as part of execution state.
- [ ] Support opening and closing positions with isolated margin using a base collateral token. 
- [ ] Enforce basic leverage limits and real-time margin checks at commit time.
- [ ] Match market orders on the execution layer using the existing price-time priority model.
- [ ] Apply mark price–based liquidation when margin falls below threshold.
- [ ] Apply periodic funding rate payments between longs and shorts.
- [ ] Implement a simple insurance fund to absorb bad debt on liquidation.
- [ ] Charge flat taker/maker fees and apply them to insurance fund or burn.


**Consensus Improvements**
- [x] Chained HotStuff: Pipelined block progress with reduced view latency.
- [ ] Integrate HotStuff 2 enhancements (optimistic responsiveness, speculative execution).
- [ ] Add block and ledger persistence to disk.

**Performance Tuning**
- [ ] Profile and optimize network throughput and view synchronization latency.
- [ ] Introduce bounded memory policies for MessageWindow under high view churn.

**Resynchronization**
- [ ] Allow replicas to resync to current ledger state on reconnect or crash recovery.

**Mempool Improvements**
- [ ] Enforce per-account pending transaction limits to defend against spam attacks.

---

## License

This project is licensed under the [MIT License](LICENSE). You’re free to modify and distribute it.

---

## References

- [HotStuff: BFT Consensus with Linear View Changes (Yin et al., 2019)](https://arxiv.org/abs/1803.05069)  
- [HotStuff 2: Simple and Optimal Consensus (Chatterjee et al., 2023)](https://arxiv.org/abs/2310.06775)  
- [Tokio Async Runtime](https://docs.rs/tokio/latest/tokio)  

---

**Disclaimer**: This is an educational project to explore a HotStuff-based BFT consensus mechanism in Rust. It is *not* production-ready.
