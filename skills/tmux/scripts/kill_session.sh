#!/bin/bash
# Kill a tmux session
# Usage: kill_session.sh <name>
SESSION_NAME="${1:?Usage: kill_session.sh <name>}"
# '=' requires an exact session name, avoiding prefix/glob target matches.
tmux kill-session -t "=$SESSION_NAME" 2>&1
