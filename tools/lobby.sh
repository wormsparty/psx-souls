#!/usr/bin/env bash
# Signaling server for online co-op (matchbox_server), on ws://0.0.0.0:3536 (or the port given).
# It only puts the players of a room in touch: the game itself then goes peer to peer.
# Installed on first launch (cargo install).
set -e
PORT="${1:-3536}"
if ! command -v matchbox_server >/dev/null 2>&1; then
    cargo install matchbox_server --version 0.14.0 --locked
fi
exec matchbox_server "0.0.0.0:$PORT"
