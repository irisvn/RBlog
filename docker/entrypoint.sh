#!/bin/sh
# RBlog's container: migrations (waiting for MySQL while it starts) and permissions, then the
# command (`serve` by default).
set -eu
cd /app
if [ "${1:-serve}" = "serve" ]; then
  tries=0
  until out=$(/app/rblog migrate 2>&1); do
    tries=$((tries + 1))
    if [ "$tries" -ge 60 ]; then
      printf '%s\n' "$out" >&2
      echo "rblog: the database is not reachable" >&2
      exit 1
    fi
    echo "rblog: waiting for the database ($tries)" >&2
    sleep 2
  done
  printf '%s\n' "$out"
  /app/rblog permission:sync
fi
exec /app/rblog "${@:-serve}"
