#!/bin/bash
# List all tmux sessions with details
tmux list-sessions -F "#{session_name}: #{session_windows} windows (#{session_id})"
