#!/bin/bash
# Lokaler Icecast im Container für `woa-headless broadcast-test` (Paket 10).
# Lauscht nur auf 127.0.0.1:8000. Die Kennwörter sind Wegwerf-Werte für diesen Test,
# entstehen hier zufällig und liegen mit Modus 600 im Laufzeitordner, nie im Repo.
#
#   tests/broadcast/icecast.sh start   # Container starten, Kennwortdatei ausgeben
#   tests/broadcast/icecast.sh stop    # Container beenden (--rm räumt ihn ab)
set -euo pipefail

NAME=woa-icecast-test
IMAGE=libretime/icecast:2.4.4
DIR="${XDG_RUNTIME_DIR:-/tmp}/woa-icecast"

random() { head -c 24 /dev/urandom | base64 | tr -d '/+=\n' | head -c 24; }

case "${1:-}" in
start)
    mkdir -p "$DIR"
    chmod 700 "$DIR"
    umask 077
    random >"$DIR/source.pass"
    {
        echo "ICECAST_SOURCE_PASSWORD=$(cat "$DIR/source.pass")"
        echo "ICECAST_ADMIN_PASSWORD=$(random)"
        echo "ICECAST_RELAY_PASSWORD=$(random)"
    } >"$DIR/env"
    docker run -d --rm --name "$NAME" -p 127.0.0.1:8000:8000 --env-file "$DIR/env" "$IMAGE" >/dev/null
    for _ in $(seq 1 50); do
        if curl -fs -o /dev/null http://127.0.0.1:8000/status-json.xsl; then
            echo "Icecast läuft auf 127.0.0.1:8000"
            echo "Kennwortdatei: $DIR/source.pass"
            exit 0
        fi
        sleep 0.2
    done
    echo "Icecast antwortet nicht; docker logs $NAME" >&2
    exit 1
    ;;
stop)
    docker stop "$NAME" >/dev/null 2>&1 || true
    rm -f "$DIR/env" "$DIR/source.pass"
    echo "Icecast beendet"
    ;;
*)
    echo "Aufruf: $0 start|stop" >&2
    exit 2
    ;;
esac
