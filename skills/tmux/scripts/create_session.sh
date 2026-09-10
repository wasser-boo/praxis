#!/bin/bash
# Create a new tmux session
# Usage: create_session.sh <name>
SESSION_NAME="${1:-default}"
# Preserve tmux's failure status; do not report a failed creation as success.
tmux new-session -d -s "$SESSION_NAME" 2>&1
