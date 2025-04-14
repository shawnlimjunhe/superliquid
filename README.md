# HotStuff-Based BFT Consensus (Work-in-Progress)

A Rust implementation of a **Byzantine Fault Tolerant (BFT)** consensus protocol inspired by [HotStuff](https://arxiv.org/abs/1803.05069) and [HotStuff 2](https://arxiv.org/abs/2310.06775). This project aims to demonstrate a rotating leader scheme, peer-to-peer networking, exponential backoff for view synchronization, and the core four-phase pipeline (Prepare → Pre-Commit → Commit → Decide).

> **Status**: **WIP** – feedback is welcome!

---

## Table of Contents

1. [Overview](#overview)  
2. [Key Features](#key-features)  
3. [Getting Started](#getting-started)  
4. [Usage](#usage)  
5. [Roadmap](#roadmap)  
6. [License](#license)  
7. [References](#references)

---

## Overview

This repository implements the fundamental components of HotStuff-style consensus:
- **Pacemaker** for handling view changes (timeouts, new-view messages, leader rotation).
- **Peer-to-Peer Network Layer** based on `tokio` for async I/O.
- **Replica Logic** featuring the HotStuff four-phase pipeline (Prepare, Pre-Commit, Commit, Decide).
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

3. **Run Tests** (If available) 
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

- [ ] **Client Transactions**: Allow clients to send token transfers between accounts.
- [ ] **State Validation**: Validate balances and persist the resulting state in each committed block.
- [ ] **Performance Tuning**: Profile and optimize message passing, QC handling, and view transitions.
- [ ] **Chained HotStuff**: Refactor the 4-phase pipeline into a pipelined (chained) HotStuff variant for improved throughput.
- [ ] **HotStuff 2 Upgrade**: Integrate enhancements from HotStuff2, including optimistic responsiveness and speculative execution.
- [ ] **State Persistence**: Write committed blocks and ledger state to disk.

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
