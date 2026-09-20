#!/bin/sh
# Praxis-Container-Entrypoint: einmalige Master-Key-Zustellung + Rechteabgabe.
#
# Sicherheitsmodell (shared VM-mode, kein qemu nötig):
#   * Compose liefert den Master-Key als root-lesbare Datei (default
#     /run/secrets/master_key). Nur ROOT kann sie lesen — die Agent-Shell
#     (uid 1000) nicht.
#   * Dieser Entrypoint läuft als root: Er kopiert den Key nach
#     /dev/shm/.praxis_master_key (tmpfs, 0400, uid 1000) und droppt dann
#     per setpriv auf den Praxis-Benutzer. Praxis liest+LÖSCHT die Datei
#     beim Start (main.rs) — sie existiert nur im Startfenster, bevor der
#     Agent Befehle ausführen kann. /proc/environ bleibt sauber, `env`
#     zeigt nichts.
#   * Restart/reboot: Entrypoint läuft erneut → Key wird erneut kopiert.
#     Der Compose-Secret-Pfad selbst bleibt dauerhaft root-only.
set -e

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

if [ "$(id -u)" = "0" ]; then
    # State-Verzeichnisse mit richtiger Ownership (auch frische Binds).
    for d in /opt/praxis/data /opt/praxis/contexts; do
        mkdir -p "$d" 2>/dev/null || true
        chown -R "$PUID:$PGID" "$d" 2>/dev/null || true
    done
    if [ -n "$SECRETS_DIR" ] && [ "$SECRETS_DIR" != "." ]; then
        mkdir -p "$SECRETS_DIR" 2>/dev/null || true
        chown -R "$PUID:$PGID" "$SECRETS_DIR" 2>/dev/null || true
    fi

    if [ -n "$MASTER_KEY_FILE" ] && [ -f "$MASTER_KEY_FILE" ]; then
        if install -m 0400 -o "$PUID" -g "$PGID" "$MASTER_KEY_FILE" /dev/shm/.praxis_master_key 2>/dev/null; then
            MASTER_KEY_FILE=/dev/shm/.praxis_master_key
            export MASTER_KEY_FILE
            echo "master key: delivered via tmpfs (deleted after read by praxis)"
        else
            # Kein beschreibbares /dev/shm oder kein root-lesbarer Key:
            # Datei unverändert durchreichen — Praxis liest direkt.
            echo "warning: master key not copied to tmpfs (delivered in place)"
        fi
    fi

    exec setpriv --reuid "$PUID" --regid "$PGID" --clear-groups "$0" "$@"
fi

# Als unprivilegierter User: Erststart/Repair der einkompilierten Assets
# (static/, Workflows, Skills …) auf die Disk — idempotent (created/preserved),
# offline, fasst keine Secrets/DB/State an. OHNE das ist ein frischer
# Container dashboard-blind (/static/app.js, /logo.png → 404; 21.09. live
# auf dem NAS gesehen — Assets liegen nicht im Image, sondern in der Binary).
/opt/praxis/bin/praxis repair-assets --directory /opt/praxis \
    || echo "warning: repair-assets failed — Dashboard evtl. ohne Assets"

# Praxis-Binary mit den CMD-Argumenten starten.
exec /opt/praxis/bin/praxis "$@"
