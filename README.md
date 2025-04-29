# HotStuff-Based BFT Consensus (Work-in-Progress)

A Rust implementation of a **Byzantine Fault Tolerant (BFT)** consensus protocol inspired by [HotStuff](https://arxiv.org/abs/1803.05069) and [HotStuff 2](https://arxiv.org/abs/2310.06775). This project aims to demonstrate a rotating leader scheme, peer-to-peer networking, exponential backoff for view synchronization, and chained hotstuff pipeline (previously core three-phase pipeline (Prepare → Pre-Commit → Commit)).

> **Status**: **WIP** – feedback is welcome!

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

This repository implements the fundamental components of HotStuff-style consensus:
- **Pacemaker** for handling view changes (timeouts, new-view messages, leader rotation).
- **Peer-to-Peer Network Layer** based on `tokio` for async I/O.
- **Replica Logic** featuring the chained HotStuff pipeline.
- **Leader Rotation** using a simple round-robin approach: `leader_id = view % num_replicas`.
- **Fault Tolerance** supporting up to `f` Byzantine nodes in a network of `3f + 1` replicas.

The purpose of this project is to learn, experiment, and demonstrate a BFT consensus mechanism in Rust. It’s **not** production-ready.

---

## Key Features

- **Async Rust Networking**: Uses `tokio` to handle concurrent connections, enabling each node to accept inbound connections and dial others.
- **Deterministic Leader Election**: A rotating leader ensures fairness and mitigates the risk of a single faulty leader stalling the protocol.
- **Exponential Timeouts**: If the protocol doesn’t make progress in a view, replicas increase their timeout before moving to the next view.
- **View Synchronization**: Fast-forward logic ensures that once any replica observes a higher-view message, it jumps to that view to maintain synchronization.
- **Eventual Safety and Liveness**: Adheres to the HotStuff design, ensuring that honest replicas eventually agree on a final sequence of blocks.

---
## Architecture

Each replica node is split into two main layers:

- **Consensus Layer**  
  Handles HotStuff logic: block proposals, voting, view advancement, QC formation, block commitment and state management.

- **Networking and Communication Layer**  
  Manages peer-to-peer TCP connections for consensus messages and client RPC handling, including automatic reconnection.

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
Committed Blocks (persisted final state)
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
- Manages transaction ordering by account nonce.
- Prioritizes urgent operations (i.e. liquidations, cancels) over normal transfers.

### MessageWindow
- Buffers Hotstuff messages per view.
- Enables efficient quorum certificate formation and view synchronization.
- Supports fast pruning of obsolete views to maintain bounded memory usage.

### LedgerState
- Validates and applies transactions atomically on committed blocks.
- Maintains nonce, balance, and account data for consistency.

---

## Design Decisions

### Consensus Model
- Implements chained HotStuff for pipelined block progress with reduced view latency.
- Enforces safety (no conflicting commits) and liveness (eventual commit under partial synchrony) by following core HotStuff invariants.

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

### MessageWindow
- Maintains recent HotStuff messages in a memory-local `VecDeque<Vec<_>>` structure, optimized for fast insertion and pruning.
- Supports non-contiguous view arrivals (i.e., missing views) without breaking indexing invariants.
- Enables constant-time lookup of messages by view number, critical for quorum checking.
- Future improvements: 
  - Deduplicate view messages by (message type, validator) to reduce memory usage.
  - Limit MessageWindow size to defend against high-frequency view changes.



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

```bash
cargo run -- node 0   // run node 0  
cargo run -- node 1   // run node 1  
cargo run -- node 2   // run node 2  
cargo run -- node 3   // run node 3  
```

### Running the Client Console
You can run a **client console** to interact with the network by starting a console instance (functionality is still barebones):

```bash
cargo run -- console 0
```

The client console lets you:

- Create/load accounts (Ed25519 keypairs)
- Request a drip (faucet funding)
- Query your account balance

The transfer functionality is under development (WIP).

  
After starting the console, type `help` to see available commands.


### Setup Keys with `.env`

To provide each node with its identity:
1. Rename the provided `.env-example` file to `.env`
2. This file contains **randomly generated private keys** for each node

Ensure each running instance has the appropriate `.env` file available in its working directory.

### Observing Logs

You can monitor the console output to see:

- **View changes** when the pacemaker times out or receives higher-view messages.
- **Block proposals** from the leader in each view.
- **Votes/QCs** exchanged between nodes.

---

## Roadmap

**Consensus Improvements**
- [x] Chained HotStuff: Pipelined block progress with reduced view latency.
- [ ] Implement transaction bundling: allow multiple sequential transactions from an account per block.
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
