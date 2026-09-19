# Juri Buzz migration validation — 19 September 2026

## Runtime

- Buzz source: `3cfd0d3` on `codex/juri-buzz-platform`
- Relay: `10.20.0.1:3300`, healthy
- Agent: `juri-root`, owner-only
- Harness: `codex-acp 1.12.0`, Codex `gpt-5.6-luna`
- Session store: SQLite WAL under the isolated Juri HOME
- Observer: NIP-AO enabled

## Live turns

| Probe | Result | Visible latency |
|---|---|---:|
| `PERSISTENCIA-OK` | passed | ~4 s |
| `RESTORE-OK` after service restart | passed | ~8 s |
| `ACTIVITY-OK` with NIP-AO observer | passed | ~4 s |

The binding retained the same ACP session ID across the restart. The relay
received additional `kind:24200` frames without increasing the WebSocket auth
rejection counter. The Juri currently has five persisted `kind:44200` events,
each addressed to the owner with a `#p` tag.

## Automated checks

- `cargo test -p buzz-acp --lib -- --test-threads=1`: **952 passed, 0 failed, 1 ignored**
- `cargo clippy -p buzz-acp -p buzz-relay --all-targets -- -D warnings`: passed
- `cargo fmt --all -- --check`: passed
- Juridico `.venv/bin/python -m pytest -q`: passed
- Juridico customs fixtures eval: verdict **pass**
- Legal capability index `--check`: passed
- Prometheus config: valid
- Prometheus target `relay:9102`: `up=1`
- Grafana health: `database=ok`
- Loki: journald/Docker Buzz streams queryable
- Tempo: relay traces queryable with `rootServiceName=buzz-relay`

## Remaining gate

The Windows host is visible as WireGuard peer `10.20.0.10`, but TCP/22 is
still closed. The PowerShell OpenSSH/firewall bootstrap is required before the
Buzz Dev NSIS build can be cloned and installed side-by-side with the official
Desktop.
