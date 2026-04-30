#!/bin/bash
# Create a new tmux session
# Usage: create_session.sh <name>
SESSION_NAME="${1:-default}"
tmux new-session -d -s "$SESSION_NAME" 2>&1 || echo "Session '$SESSION_NAME' already exists or failed to create"
