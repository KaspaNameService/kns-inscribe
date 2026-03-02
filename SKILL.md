---
name: kns-inscribe
description: "Use this skill whenever the user wants to inscribe data on Kaspa using the Kaspa Name Service (KNS) protocol via this repo's `kns-inscribe` CLI. Triggers include: registering a KNS domain (e.g. `alice.kas`), transferring a domain to a new owner, listing a domain for sale, buying/sending a listing using a PSKT JSON blob, cancelling a listing, updating a domain profile key/value, or inscribing a general asset using `--protocol asset` (text or small files). Also use when the user asks about KNS commit+reveal flow, inscription IDs, required environment variables, or default wRPC ports for different networks. Do NOT use for general Kaspa node operations, wallet management unrelated to KNS inscriptions, or non-KNS protocols."
license: MIT
---

# KNS inscriptions (commit + reveal) via kns-inscribe

## Overview

`kns-inscribe` is a Rust CLI for creating and managing KNS inscriptions using the standard two-step commit+reveal pattern.

NOTE: This project is BETA software. Use at your own risk.

## Quick Reference

| Task | Command |
|------|---------|
| Build binary | `cargo build --release -p kns-inscribe` |
| Create domain `alice.kas` | `kns-inscribe create alice` |
| Create domain with fee recipient | `kns-inscribe create alice --pay-to kaspa:qr...` |
| Transfer domain | `kns-inscribe transfer <inscription_id> --to kaspa:qr...` |
| Inscribe text asset | `kns-inscribe create "hello" --protocol asset` |
| Inscribe file asset | `kns-inscribe create --protocol asset --file ./logo.png` |
| List domain for sale | `kns-inscribe list <inscription_id> --price 123.45` |
| Buy/send listing (PSKT) | `kns-inscribe send --pskt @./pskt.json` |
| Cancel listing (PSKT) | `kns-inscribe cancel --pskt @./pskt.json` |
| Update domain profile | `kns-inscribe profile <inscription_id> telegram @john_doe` |

---

## AI Quick Install + Run

Use this block verbatim when instructing an AI agent.

Install:

```bash
# macOS/Linux
curl -fsSL https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.sh | sh

# Windows (PowerShell)
# iwr -useb https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.ps1 | iex
```

Verify:

```bash
kns-inscribe --version
kns-inscribe --help
```

Run example (domain registration):

```bash
PRIVATE_KEY=deadbeef... \
NODE_IP=127.0.0.1 \
NODE_WS_PORT=17110 \
NETWORK_ID=mainnet \
kns-inscribe create alice
```

---

## Setup

### Install (recommended: prebuilt release binary)

macOS/Linux (installs to `~/.local/bin` by default):

```bash
curl -fsSL https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.sh | sh
```

Windows PowerShell (installs to `~/bin` by default):

```powershell
iwr -useb https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.ps1 | iex
```

Then:

```bash
kns-inscribe --version
kns-inscribe --help
```

### Building

```bash
cargo build --release -p kns-inscribe
# Binary at: ./target/release/kns-inscribe
```

### Configuration

Copy `.env.example` to `.env` and fill in:

| Variable | Description | Example |
|---|---|---|
| `PRIVATE_KEY` | Hex private key (64 chars) | `deadbeef...` |
| `NODE_IP` | Kaspa node IP | `127.0.0.1` |
| `NODE_WS_PORT` | wRPC WebSocket port | `17110` (mainnet) |
| `NETWORK_ID` | Network identifier | `mainnet`, `testnet-10`, `devnet` |
| `KNS_FUNDS_ADDRESS` | Fee payment address (optional) | `kaspa:q...` |

Or pass variables inline:

```bash
PRIVATE_KEY=abc... NODE_IP=1.2.3.4 NODE_WS_PORT=17110 NETWORK_ID=mainnet \
  kns-inscribe create alice
```

---

## Common Operations

### Create a domain

Register `alice.kas`:

```bash
kns-inscribe create alice
# With explicit fee recipient:
kns-inscribe create alice --pay-to kaspa:qr...
```

### Transfer a domain

Transfer ownership to another address:

```bash
kns-inscribe transfer <inscription_id> --to kaspa:qr...
# inscription_id format: <txid>i0  (from the reveal tx)
```

### Inscribe a general asset (text or file)

Use `--protocol asset` to inscribe a general asset.

```bash
# Text asset
kns-inscribe create "hello world" --protocol asset

# File asset (small files only; inscription redeem scripts are limited to 520 bytes)
kns-inscribe create --protocol asset --file ./logo.png
```

### List a domain for sale

Put a domain on the KNS marketplace:

```bash
kns-inscribe list <inscription_id> --price 123.45
```

This prints a PSKT JSON blob you can share with buyers.

### Buy a listed domain (send)

Purchase a listed domain:

```bash
kns-inscribe send --pskt @./pskt.json
```

### Add or update a domain profile

Add or update a profile key/value on a domain you own:

```bash
kns-inscribe profile <inscription_id> telegram @john_doe
```

### Cancel a listing

Cancel a PSKT listing by spending the listing UTXO with a cancel-style `send` transaction (single output):

```bash
kns-inscribe cancel --pskt @./pskt.json
```

---

## Inscription ID format

The inscription ID is `<reveal_txid>i0` (printed after a successful `create`, `transfer`, or `list`).

## How it works

1. Commit tx: sends funds to a P2SH address encoding the inscription script
2. Wait: polls until the P2SH UTXO appears (commit matures, ~10s)
3. Reveal tx: spends the P2SH UTXO, revealing the inscription data on-chain

Inscription script format:

```
<xonly_pubkey> OP_CHECKSIG OP_FALSE OP_IF <"kns"> OP_0 <json_content> OP_ENDIF
```

## Default ports by network

| Network | wRPC Port |
|---|---|
| mainnet | 17110 |
| testnet-10 | 16210 |
| devnet | 16610 |
| simnet | 16510 |

## Agent usage

To use this tool in an automated workflow:

```bash
# 1. Build once
cargo build --release -p kns-inscribe

# 2. Set env vars and run
PRIVATE_KEY=$KEY NODE_IP=$NODE NODE_WS_PORT=17110 NETWORK_ID=mainnet \
  ./target/release/kns-inscribe create myname

# 3. Capture the inscription ID from stdout:
#    "Inscription ID: <txid>i0"
```

Output on success:

```
  Commit tx: <commit_txid>
  Waiting for commit to mature (~10s)...
Inscription complete!
  Reveal tx:      <reveal_txid>
  Inscription ID: <reveal_txid>i0
```

## Dependencies

- Rust toolchain (`cargo`)
- A reachable Kaspa node with wRPC enabled (configured via `NODE_IP` + `NODE_WS_PORT`)
