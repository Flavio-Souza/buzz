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

## Windows Desktop validation

- SSH: `DESKTOP-IF88B2A\flavi@10.20.0.10`
- Fork clone: `D:\Work\iaquant\LAB\buzz`
- Build commit: `fa08bc0`
- Node: `22.18.0`
- pnpm: `11.4.0`
- Rust target: `x86_64-pc-windows-msvc`
- Rust toolchain used by the workspace: `1.95.0`
- Installer: `Buzz Dev_0.5.23_x64-setup.exe`
- Installer SHA-256: `58FA6CF9D8F7CEB45CA69962FAE2136F91BE87413080EA48DDD7B628DE84B7A2`
- Installed product: `Buzz Dev 0.5.23`
- Installed path: `C:\Users\flavi\AppData\Local\Buzz Dev`
- Official Buzz remains installed at `C:\Users\flavi\AppData\Local\Buzz`.

The installer and side-by-side registration succeeded. The remaining human
step is opening Buzz Dev in the interactive Windows desktop session, importing
the owner identity through native pairing, connecting to `ws://10.20.0.1:3300`
and opening Activity for the Juri DM/channel. The server-side observer path is
already proven by the NIP-AO counters above.
