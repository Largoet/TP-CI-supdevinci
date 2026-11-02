#!/usr/bin/env bash

if [ $# -ne 2 ]; then
  echo "Usage: $0 <FROM> <TO>"
  exit 1;
fi

from=$1
to=$2

rg -n "$from" -l | xargs sed -i "s/$from/$to/g"
