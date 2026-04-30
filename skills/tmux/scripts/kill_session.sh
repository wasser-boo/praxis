#!/bin/bash
# Kill a tmux session
# Usage: kill_session.sh <name>
SESSION_NAME="${1:?Usage: kill_session.sh <name>}"
tmux kill-session -t "$SESSION_NAME" 2>&1
