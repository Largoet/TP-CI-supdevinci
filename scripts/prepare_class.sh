#!/usr/bin/env bash

if ! REPO=$(git rev-parse --show-toplevel); then
  echo "Not executed inside the repo"
  exit 1
fi

set -e
rm -rf /tmp/prepare-simeis-class
git clone "file://$REPO" --depth=1 /tmp/prepare-simeis-class
cd /tmp/prepare-simeis-class
rm -rf .git*
rm -rf .forgejo
rm flake.*
zip -9 -r "$REPO/simeis-class.zip" *
cd -

ls -lh "$REPO/simeis-class.zip"
