#!/usr/bin/env sh

echo "---- update nix rust dependency ----"
nix flake update fenix --commit-lock-file
