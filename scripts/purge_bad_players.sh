#!/usr/bin/env bash

MAX_AGE=$((60 * 5))

if [ -z "$SIMEIS_ADMIN_TOKEN" ]; then
  echo "SIMEIS_ADMIN_TOKEN env variable not available"
  exit 1;
fi

if [ $# -ne 2 ]; then
  echo "Usage: $0 <IP> <PORT>"
  exit 1;
fi

URL="http://$1:$2"

set -e
if ! all_players=$(curl -s -f "$URL/gamestats"); then
  echo "Failed to get stats from server"
fi
echo "$all_players" | jq
if ! IDS_RM=$(echo "$all_players"|jq -r "to_entries[]
  | select(.key != \"error\")
  | select(.value.lost or ((.value.age > $MAX_AGE) and (.value.score == 0.0)))
  | .key
"); then
  echo "Failed to get ID of lost players, or stuck at 0.0 score"
fi
echo "$IDS_RM"

for id in $IDS_RM; do
  echo "Removing player of ID $id"
  curl \
    -H "Simeis-Admin-Token: $SIMEIS_ADMIN_TOKEN" \
    -X POST -s -f \
    "$URL/admin/player/remove/$id" || echo "Failed to remove player $id"
  echo ""
done
