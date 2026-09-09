# Anchor

A Stellar anchor **business server** written in Rust, running behind the
SDF's `stellar/anchor-platform` reference image. The platform owns the SEP
wire protocol (SEP-1 discovery, SEP-10 auth, SEP-6/24/31/38 endpoints);
anchor-rust owns everything domain-specific — KYC data, pricing, and real
custody (it holds a testnet distribution account, signs and submits real
Stellar payments, and observes incoming payments via a live Horizon
streaming subscription). There is no real banking integration, so the fiat
leg of every flow is simulated/logged.

This used to be a from-scratch anchor that implemented SEP-1/10/12/24/31
itself, with no platform in front of it — useful at the time for
understanding SEP protocol mechanics at the wire/crypto level. It's now
rebuilt to match the architecture a production anchor actually uses: the
platform for protocol, this service for business logic and custody. See
`git log` for the prior single-process version if you need the old
reference.

## Architecture

```
  wallet / sending anchor
          |
          v
  +--------------------+      callbacks: /customer /rate /event    +----------------------+
  |  Anchor Platform    |  ------------------------------------->  |  anchor-rust          |
  |  (SEP server +      |                                          |  (this service)       |
  |   Platform API +    |  <-------------------------------------  |                       |
  |   Stellar Observer) |     Platform API: notify_* / PATCH        |  ledger + observer    |
  +--------------------+                                          |  (custody, real keys)  |
                                                                    +----------------------+
```

- **The platform** serves `/.well-known/stellar.toml`, `/auth` (SEP-10),
  `/sep12/customer`, `/sep24/*`, `/sep31/*`, `/sep38/*` — the full wallet-
  facing wire protocol.
- **anchor-rust** answers three callbacks (`/customer`, `/rate`, `/event`),
  hosts its own SEP-24 interactive UI (`/sep24/interactive`), and reports
  money movement back to the platform via its Platform API (`notify_*`
  JSON-RPC calls, and one `PATCH /transactions/{id}` call for assigning a
  withdrawal/receive destination).
- **Custody stays here**, not in the platform. The platform never holds a
  Stellar key for payments; `ledger.rs` signs and submits every payment,
  `observer.rs` watches Horizon for incoming ones. This is the "custody
  server" pattern (as opposed to letting the platform sign payouts itself
  from a key in its own env) — the same split a production business server
  should use.

## Running it locally

Requirements: Rust (stable), Docker, the [Stellar CLI](https://github.com/stellar/stellar-cli), a Stellar testnet keypair.

1. Start anchor-rust's own Postgres:
   ```
   docker run -d --name anchor-rust-pg -e POSTGRES_PASSWORD=postgres \
     -e POSTGRES_DB=anchor_rust -p 5460:5432 postgres:15.2-alpine
   ```
2. Copy `.env.example` to `.env` (see below for the required variables) and
   generate/fund a distribution keypair if you don't have one yet
   (`cargo run --example smoke_payment` will do it implicitly, or use any
   Stellar SDK + `https://friendbot.stellar.org/?addr=<G...>`).
3. Bring up the anchor platform:
   ```
   cd local/anchor-platform && ./ap_start.sh
   ```
   This generates the platform's own SEP-10 signing keypair (separate from
   your distribution key — the platform authenticates wallets, it doesn't
   move money), templates `assets.yaml`/`stellar.localhost.toml`, and starts
   the platform + Kafka + the platform's own Postgres via Docker Compose.
4. In a second terminal, run anchor-rust itself:
   ```
   cargo run
   ```
5. `curl http://localhost:8080/.well-known/stellar.toml` should return the
   anchor's TOML file (served by the platform).

### Required environment variables (anchor-rust's own `.env`)

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | Postgres connection string (anchor-rust's own DB, separate from the platform's) |
| `DISTRIBUTION_SEED` | Custody keypair — signs and sends every payment (`S...`) |
| `SECRET_SEP24_INTERACTIVE_URL_JWT_SECRET` | Shared with the platform — verifies the interactive session URL the platform mints |
| `SECRET_CALLBACK_API_AUTH_SECRET` | Shared with the platform — verifies inbound `/customer` `/rate` `/event` calls are really from the platform |
| `SECRET_PLATFORM_API_AUTH_SECRET` | Shared with the platform — authenticates anchor-rust's outbound Platform API calls |

Optional (defaults shown): `SERVER_ADDR=0.0.0.0:8080`, `BASE_URL=http://localhost:8080`,
`HORIZON_URL=https://horizon-testnet.stellar.org`,
`NETWORK_PASSPHRASE=Test SDF Network ; September 2015`, `ASSET_CODE=TEST`,
`PLATFORM_API_BASE_URL=http://localhost:8085`.

The three shared secrets must match the corresponding values in
`local/anchor-platform/dev.env` — see that file's comments.

## Verifying it end to end

The `examples/` directory still doubles as a set of real-testnet integration
checks (no mocks — every one signs and submits actual Stellar transactions),
now driven through the platform's endpoints (`:8080`) rather than
anchor-rust's own:

```
cargo run --example smoke_payment    # ledger client: fund, trust, pay
cargo run --example smoke_sep12      # SEP-10 login + SEP-12 customer CRUD, via the platform
cargo run --example smoke_sep24      # full deposit + withdrawal, via the platform + our interactive UI
cargo run --example smoke_sep31      # receive with and without a KYC gate, via the platform
```

Run them against both the platform (`./local/anchor-platform/ap_start.sh`)
and anchor-rust (`cargo run`) running.

Pure-logic unit tests (fee math, required-field tables, interactive-token
round trips, account-id parsing): `cargo test`.

## What's implemented

- **Callbacks**: `/customer` (SEP-12 get/put/delete), `/rate` (SEP-38
  indicative + firm — flat 10% fee, no real FX since this anchor issues one
  asset at a fixed price), `/event` (`transaction_created` for SEP-24/31,
  allocating a withdrawal/receive memo and custody destination).
- **Custody**: real Horizon SSE payment observation with memo-based
  matching, real payment signing/submission, all unchanged from the original
  single-process build.
- **SEP-24 interactive UI**: hosted here (`sep24/interactive.rs`, askama
  templates), linked from the URL the platform hands back to the wallet.
- **SEP-12 KYC**: required-field-by-customer-type table (`kyc.rs`),
  ownership no longer double-checked here — the platform already verified
  the wallet's SEP-10 identity before ever calling us.

## Known limitations / open questions from this rebuild

- **`notify_onchain_funds_received` and `notify_offchain_funds_sent`
  parameter shapes are not yet confirmed** against a running
  `stellar/anchor-platform` image — they're implemented by inference from
  `notify_onchain_funds_sent` / `notify_offchain_funds_received` (which
  *are* confirmed, against a working `anchor-ms` integration) and the
  platform's own JSON-RPC methods reference. Verify before relying on them.
- **The withdrawal/receive destination assignment uses `PATCH
  /transactions/{id}`**, the older documented mechanism, rather than the
  newer `GET /unique-address` pull-based callback also documented for the
  same purpose. This project picked one consistent, push-based mechanism
  across SEP-24 and SEP-31 rather than implementing both without a live
  platform to verify either against — confirm which your installed platform
  version actually expects.
- **No SEP-38 quotes beyond the flat fee** — same scope cut as before, now
  exposed through a real `/rate` callback instead of being computed inline.
- **One keypair still plays both custody roles** (signing/distribution) —
  a real deployment separates hot and cold keys, and probably backs the hot
  key with a vault rather than an env var.
- **The payment observer still has no persisted cursor** — a restart misses
  payments received while down.
- **The fiat leg of every flow is simulated** (logged, not actually
  transferred).
