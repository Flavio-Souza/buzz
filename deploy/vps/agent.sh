#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AGENTS_DIR="${SCRIPT_DIR}/agents"
SERVICE_TEMPLATE="${SCRIPT_DIR}/buzz-agent@.service"

usage() {
    cat <<'MSG'
Usage: ./agent.sh create|keygen|auth|start|stop|restart|logs|status|config|rm <name>
       ./agent.sh add-direct-member <npub-or-hex>

Agents run on the host under systemd. Configuration lives in
agents/<name>/agent.env and agents/<name>/instructions.md.

auth runtimes: codex or claude (copies only that runtime's login file into the
agent HOME; use a separate API key instead when stronger credential isolation
is required).

Agents with BUZZ_AUTH_TAG authenticate through their NIP-OA owner. Do not add
those agent pubkeys as direct relay members: doing so bypasses owner
materialization and breaks owner-scoped observer/usage events. add-direct-member
is only for humans or legacy identities without NIP-OA.
MSG
}

valid_name() {
    [[ "${1:-}" =~ ^[a-z0-9][a-z0-9_-]*$ ]]
}

agent_dir() {
    printf '%s/%s' "$AGENTS_DIR" "$1"
}

relay_compose() {
    docker compose --env-file "${SCRIPT_DIR}/.env" \
        -f "${SCRIPT_DIR}/compose.yaml" exec -T relay "$@"
}

install_service_template() {
    sudo -n install -m 644 "$SERVICE_TEMPLATE" /etc/systemd/system/buzz-agent@.service
    sudo -n systemctl daemon-reload
}

name="${2:-}"
case "${1:-help}" in
    create)
        valid_name "$name" || { echo "Invalid agent name: use lowercase letters, numbers, _ or -." >&2; exit 2; }
        dir="$(agent_dir "$name")"
        if [[ -e "$dir" ]]; then
            echo "Agent directory already exists: $dir" >&2
            exit 1
        fi
        install -d -m 700 "$dir/home" "$dir/workspace"
        install -m 600 "${AGENTS_DIR}/agent.env.example" "$dir/agent.env"
        install -m 644 "${AGENTS_DIR}/instructions.md" "$dir/instructions.md"
        sed -i \
            -e "s#^AGENT_NAME=.*#AGENT_NAME=${name}#" \
            -e "s#^AGENT_DIR=.*#AGENT_DIR=${dir}#" \
            -e "s#^BUZZ_ACP_SYSTEM_PROMPT_FILE=.*#BUZZ_ACP_SYSTEM_PROMPT_FILE=${dir}/instructions.md#" \
            -e "s#^BUZZ_ACP_SESSION_TITLE=.*#BUZZ_ACP_SESSION_TITLE=${name}#" \
            "$dir/agent.env"
        echo "Created $dir. Set BUZZ_PRIVATE_KEY, runtime, and credentials in agent.env."
        ;;
    keygen)
        valid_name "$name" || { usage >&2; exit 2; }
        dir="$(agent_dir "$name")"
        [[ -f "$dir/agent.env" ]] || { echo "Missing agent config: $dir/agent.env" >&2; exit 1; }
        if [[ -e "$dir/identity.txt" ]]; then
            echo "Identity file already exists: $dir/identity.txt" >&2
            exit 1
        fi
        umask 077
        relay_compose buzz-admin generate-key > "$dir/identity.txt"
        chmod 600 "$dir/identity.txt"
        echo "Generated identity at $dir/identity.txt. Copy its secret key to agent.env and provision a BUZZ_AUTH_TAG signed by a relay-member owner before start. Do not add the agent as a direct relay member."
        ;;
    auth)
        valid_name "$name" || { usage >&2; exit 2; }
        runtime="${3:-}"
        dir="$(agent_dir "$name")"
        [[ -f "$dir/agent.env" ]] || { echo "Missing agent config: $dir/agent.env" >&2; exit 1; }
        case "$runtime" in
            codex)
                [[ -r "$HOME/.codex/auth.json" ]] || { echo "Missing host Codex auth: $HOME/.codex/auth.json" >&2; exit 1; }
                install -d -m 700 "$dir/home/.codex"
                install -m 600 "$HOME/.codex/auth.json" "$dir/home/.codex/auth.json"
                ;;
            claude)
                [[ -r "$HOME/.claude/.credentials.json" ]] || { echo "Missing host Claude auth: $HOME/.claude/.credentials.json" >&2; exit 1; }
                install -d -m 700 "$dir/home/.claude"
                install -m 600 "$HOME/.claude/.credentials.json" "$dir/home/.claude/.credentials.json"
                ;;
            *) echo "Runtime must be codex or claude." >&2; exit 2 ;;
        esac
        echo "Copied $runtime login state into $dir/home; credentials are not printed."
        ;;
    add-member)
        echo "Refusing ambiguous add-member. Agents with BUZZ_AUTH_TAG must enter through NIP-OA. Use add-direct-member only for a human or legacy non-NIP-OA identity." >&2
        exit 2
        ;;
    add-direct-member)
        pubkey="${2:-}"
        if [[ ! "$pubkey" =~ ^(npub1[a-z0-9]+|[0-9a-fA-F]{64})$ ]]; then
            echo "Expected an npub or 64-character hex public key." >&2
            exit 2
        fi
        relay_compose buzz-admin add-member --pubkey "$pubkey"
        ;;
    start|stop|restart|logs|status|config)
        valid_name "$name" || { usage >&2; exit 2; }
        dir="$(agent_dir "$name")"
        [[ -f "$dir/agent.env" ]] || { echo "Missing agent config: $dir/agent.env" >&2; exit 1; }
        install_service_template
        case "$1" in
            start) sudo -n systemctl enable --now "buzz-agent@${name}.service" ;;
            stop) sudo -n systemctl stop "buzz-agent@${name}.service" ;;
            restart) sudo -n systemctl restart "buzz-agent@${name}.service" ;;
            logs) sudo -n journalctl -u "buzz-agent@${name}.service" -f ;;
            status) sudo -n systemctl status "buzz-agent@${name}.service" --no-pager ;;
            config) sudo -n systemctl cat "buzz-agent@${name}.service" ;;
        esac
        ;;
    rm)
        valid_name "$name" || { usage >&2; exit 2; }
        dir="$(agent_dir "$name")"
        [[ -f "$dir/agent.env" ]] || { echo "Missing agent config: $dir/agent.env" >&2; exit 1; }
        sudo -n systemctl disable --now "buzz-agent@${name}.service" 2>/dev/null || true
        echo "Agent stopped and disabled. Host data remains at $dir; remove it manually only after backup."
        ;;
    help|-h|--help) usage ;;
    *) usage >&2; exit 2 ;;
esac
