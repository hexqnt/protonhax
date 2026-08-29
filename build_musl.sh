#!/usr/bin/env bash

set -euo pipefail

docker pull clux/muslrust:nightly
docker run \
    --user "$(id -u):$(id -g)" \
    --volume "$PWD:/volume" \
    --env CARGO_HOME=/tmp/cargo \
    --rm \
    --tty \
    clux/muslrust:nightly cargo build --release
