# SKILL.md — kns-inscribe agent instructions

This file is intended for AI agents. Follow these instructions to install and operate `kns-inscribe`.

---

## 1. Install the binary

Prefer the prebuilt binary. Do NOT build from source unless explicitly asked.

**macOS / Linux:**
```bash
curl -fsSL https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.sh | sh
```

**Windows (PowerShell):**
```powershell
iwr -useb https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.ps1 | iex
```

Verify:
```bash
kns-inscribe --version
```

If the binary is already installed and `--version` succeeds, skip installation.

---

## 2. Configure environment variables

Required variables (all must be set before any command):

| Variable       | Description                          | Example              |
| -------------- | ------------------------------------ | -------------------- |
| `PRIVATE_KEY`  | Hex-encoded private key (64 chars)   | `deadbeef...`        |
| `NODE_IP`      | Kaspa node IP or hostname            | `127.0.0.1`          |
| `NODE_WS_PORT` | wRPC WebSocket port (see table below)| `17110`              |
| `NETWORK_ID`   | Network identifier                   | `mainnet`            |

Optional:

| Variable            | Description                                    |
| ------------------- | ---------------------------------------------- |
| `KNS_FUNDS_ADDRESS` | Fee recipient address; defaults to sender      |

Default wRPC ports by network:

| Network    | Port  |
| ---------- | ----- |
| mainnet    | 17110 |
| testnet-10 | 16210 |
| devnet     | 16610 |
| simnet     | 16510 |

**Always pass `--env-file`** — never rely on `.env` auto-discovery or inline env vars:

```bash
kns-inscribe --env-file /path/to/.env create alice
kns-inscribe --env-file ~/.config/kns/mainnet.env create alice
```

If the user has not provided an env file path, ask for it before running any command.

---

## 3. Commands

### Register a domain

```bash
kns-inscribe create <name>
# e.g. registers alice.kas
kns-inscribe create alice

# with explicit fee recipient
kns-inscribe create alice --pay-to kaspa:qr...
```

Registration fees:

| Characters | Fee      |
| ---------- | -------- |
| 1–2        | 4200 KAS |
| 3          | 2100 KAS |
| 4          | 525 KAS  |
| 5+         | 35 KAS   |

### Inscribe a general asset

```bash
# text
kns-inscribe create "hello world" --protocol asset

# file (≤520 bytes after encoding)
kns-inscribe create --protocol asset --file ./logo.png
```

### Transfer a domain

```bash
kns-inscribe transfer <inscription_id> --to kaspa:qr...
```

### Add or update a domain profile key

```bash
kns-inscribe profile <inscription_id> <key> <value>
# e.g.
kns-inscribe profile d4e5f6...i0 telegram @john_doe
```

### List a domain for sale

```bash
kns-inscribe list <inscription_id> --price <KAS>
# Outputs a PSKT JSON blob — save it for the buyer
```

### Buy a listed domain

```bash
kns-inscribe send --pskt @./pskt.json

# optionally direct domain to a different address
kns-inscribe send --pskt @./pskt.json --to kaspa:qr...
```

### Cancel a listing

```bash
kns-inscribe cancel --pskt @./pskt.json
```

---

## 4. Inscription IDs

Every successful `create`, `transfer`, or `list` prints an **inscription ID** in the format `<reveal_txid>i0`. Save this — it is required for subsequent `transfer`, `list`, `profile`, etc.

---

## 5. How the commit+reveal flow works

1. **Commit** — CLI sends 0.3 KAS to a P2SH address encoding the inscription script.
2. **Wait** — CLI polls the node until the UTXO matures (~10 s).
3. **Reveal** — CLI spends the P2SH UTXO, publishing the inscription on-chain.

Both steps happen automatically. No manual intervention needed.

---

## 6. Debugging

```bash
RUST_LOG=kns_inscribe=debug kns-inscribe <command>
```

---

## 7. Agent checklist

Before running any command, verify:
- [ ] `kns-inscribe --version` succeeds
- [ ] User has provided an env file path
- [ ] Node is reachable at `ws://<NODE_IP>:<NODE_WS_PORT>`
- [ ] Wallet has sufficient KAS (registration fee + 0.3 KAS commit anchor + miner fees)
- [ ] Miner fees are computed dynamically from the live network feerate — budget extra KAS on congested networks
