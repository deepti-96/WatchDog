#!/usr/bin/env bash
set -euo pipefail

DEPLOY_ID="${1:-$(git rev-parse --short HEAD)}"
STATE_DIR="${WATCHDOG_STATE_DIR:-.watchdog}"

cargo run -- notify --state-dir "$STATE_DIR" --deploy "$DEPLOY_ID" --environment production
