#!/usr/bin/env bash

set -euo pipefail

workspace_crates=(bin/mev mev-boost-rs mev-relay-rs mev-build-rs mev-rs)

if [ $# -ne 2 ]
then
    echo "Usage: ./$(basename $0) <dependency> <commit>"
    exit 1
fi

dependency=$1
commit=$2

for workspace_crate in ${workspace_crates[@]}
do
    echo "updating $dependency dependency of $workspace_crate"
    sed -i.bak -e "s/^$dependency = \(.*\), *rev *= *\".*\" *}/$dependency = \1, rev = \"$commit\" }/" "$workspace_crate/Cargo.toml"
    rm -f "$workspace_crate/Cargo.toml.bak"
done

