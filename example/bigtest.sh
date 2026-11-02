#!/usr/bin/env bash

if [ $# -ne 2 ]; then
    echo "Usage: $0 <num players> <port>"
    exit 1
fi
NPLAYERS=$1
PORT=$2
mkdir -p bigtest/logs
cd bigtest
for i in $(seq 1 $NPLAYERS); do
    # sleep 2
    python3 ../python/client.py "player$i" 0.0.0.0 "$PORT" 1>"./logs/$i.out" 2>"./logs/$i.err" &
    echo "Started player $i"
done

python3 ../watch_game.py 0.0.0.0 "$PORT"
kill $(jobs -p)
