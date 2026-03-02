# kns-inscribe

A Rust CLI for inscribing on the [Kaspa Name Service (KNS)](https://knsdomains.org) protocol.

NOTE: This project is BETA software. Use at your own risk. It may contain bugs and may create transactions
that you did not intend. Always review and test with small amounts first.

Supports domain registration, transfers, marketplace listings, and purchases — compiled to a single native binary with no runtime dependencies.

## Installation

### From source

Requires [Rust](https://rustup.rs) 1.75+.

```bash
git clone <repo>
cd kns-inscribe
cargo build --release
# Binary: ./target/release/kns-inscribe
```

Optionally install system-wide:

```bash
cargo install --path .
```

## Configuration

Copy `.env.example` to `.env` and fill in your values:

```bash
cp .env.example .env
```

| Variable            | Required | Description                               | Example                           |
| ------------------- | -------- | ----------------------------------------- | --------------------------------- |
| `PRIVATE_KEY`       | Yes      | Hex-encoded private key (64 chars)        | `deadbeef...`                     |
| `NODE_IP`           | Yes      | Kaspa node IP or hostname                 | `127.0.0.1`                       |
| `NODE_WS_PORT`      | Yes      | wRPC WebSocket port                       | `17110`                           |
| `NETWORK_ID`        | Yes      | Network identifier                        | `mainnet`, `testnet-10`, `devnet` |
| `KNS_FUNDS_ADDRESS` | No       | Fee recipient address; defaults to sender | `kaspa:q...`                      |

Variables can also be passed inline:

```bash
PRIVATE_KEY=abc123... NODE_IP=1.2.3.4 kns-inscribe create alice
```

### Default wRPC ports

| Network    | Port  |
| ---------- | ----- |
| mainnet    | 17110 |
| testnet-10 | 16210 |
| devnet     | 16610 |
| simnet     | 16510 |

## Usage

```
kns-inscribe <COMMAND>

Commands:
  create    Register a new KNS domain
  transfer  Transfer a domain to another address
  list      List a domain for sale
  send      Buy a listed domain
  cancel    Cancel a PSKT listing
  profile   Add or update a domain profile
  help      Print help
```

### Register a domain

```bash
kns-inscribe create alice
# Registers alice.kas

kns-inscribe create alice --pay-to kaspa:qr...
# Send the registration fee to a specific address
```

Registration fees (in KAS) are based on domain length:

| Characters | Fee      |
| ---------- | -------- |
| 1          | 4200 KAS |
| 2          | 4200 KAS |
| 3          | 2100 KAS |
| 4          | 525 KAS  |
| 5+         | 35 KAS   |

### Inscribe a general asset (text or file)

Use `--protocol asset` to inscribe a general asset.

```bash
# Text asset
kns-inscribe create "hello world" --protocol asset

# File asset (small files only; inscription redeem scripts are limited to 520 bytes)
kns-inscribe create --protocol asset --file ./logo.png
```

### Transfer a domain

```bash
kns-inscribe transfer <inscription_id> --to kaspa:qr...
```

### Add or update a domain profile

Set a profile key/value on a domain you own:

```bash
kns-inscribe profile <inscription_id> telegram @john_doe
```

### List a domain for sale

```bash
kns-inscribe list <inscription_id> --price 123.45
```

The CLI prints a PSKT JSON blob you can share with buyers.

### Buy a listed domain

```bash
kns-inscribe send --pskt @./pskt.json

# Optional: set a different receiver for the domain
kns-inscribe send --pskt @./pskt.json --to kaspa:qr...
```

### Cancel a listing

Cancellation is performed by spending the listing UTXO with a `send` inscription that has only a
single output (indexers interpret this as a cancellation).

```bash
kns-inscribe cancel --pskt @./pskt.json
```

The `inscription_id` is printed after a successful `create`, `transfer`, or `list` — it has the format `<reveal_txid>i0`.

## How it works

KNS uses a two-step **commit + reveal** inscription pattern:

1. **Commit** — sends 0.3 KAS to a P2SH address that encodes the inscription script
2. **Wait** — polls the Kaspa node until the P2SH UTXO matures (~10 seconds)
3. **Reveal** — spends the P2SH UTXO, publishing the inscription on-chain

The inscription is embedded in a tapscript-style envelope:

```
<xonly_pubkey> OP_CHECKSIG
OP_FALSE OP_IF
  <"kns">
  OP_0
  <json_payload>
OP_ENDIF
```

Example payloads:

```json
{ "op": "create",   "p": "domain", "v": "alice" }
{ "op": "transfer", "p": "domain", "id": "<txid>i0", "to": "kaspa:q..." }
{ "op": "list",     "p": "domain", "id": "<txid>i0" }
{ "op": "send",                    "id": "<txid>i0" }
```

## Example output

```
Sender address: kaspa:qr...
Network:        mainnet
RPC endpoint:   ws://127.0.0.1:17110
Connected to Kaspa node.

Operation: create domain 'alice'
Registration fee: 35.0000 KAS
Inscription JSON: {"op":"create","p":"domain","v":"alice"}

  Commit tx: a1b2c3...
  Waiting for commit to mature (~10s)...

Inscription complete!
  Reveal tx:      d4e5f6...
  Inscription ID: d4e5f6...i0
```

## Logging

Set `RUST_LOG=kns_inscribe=debug` for verbose output including raw script bytes and P2SH addresses.

## Fees

This CLI currently uses hard-coded miner fees per transaction for simplicity (commit + reveal).
If the network is congested and transactions are rejected for low fees, you may need to increase
the constants in `src/inscribe.rs`.

## License

MIT
