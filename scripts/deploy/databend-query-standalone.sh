#!/bin/bash
# Copyright 2020-2021 The Databend Authors.
# SPDX-License-Identifier: Apache-2.0.

SCRIPT_PATH="$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)"
cd "$SCRIPT_PATH/../.." || exit

killall databend-store
killall databend-query
sleep 1

BIN=${1:-debug}

echo 'Start DatabendStore...'
nohup target/${BIN}/databend-store --single=true --log-level=ERROR &
echo "Waiting on databend-store 10 seconds..."
python scripts/ci/wait_tcp.py --timeout 5 --port 9191


echo 'Start DatabendQuery...'
nohup target/${BIN}/databend-query -c scripts/deploy/config/databend-query-node-1.toml &
echo "Waiting on databend-query 10 seconds..."
python scripts/ci/wait_tcp.py --timeout 5 --port 3307
