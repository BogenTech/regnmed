#!/usr/bin/env bash
# Installs Ubuntu packages on a GitHub runner, surviving an unreachable
# Ubuntu archive.
#
#     scripts/ci-apt-install.sh libxml2-utils
#     scripts/ci-apt-install.sh musl-tools
#
# 2026-11-11 the runners resolved packages.microsoft.com but not
# *.ubuntu.com for hours, and every workflow that installed anything went
# red before our code ran. So: the runner's own mirrors first, with short
# timeouts; if that fails, the SAME packages from a mirror on another
# domain. apt still checks Ubuntu's signature on the release files
# (signed-by the ubuntu-archive keyring), so the mirror is a transport and
# not something we have to trust.
set -euo pipefail

opts=(-o Acquire::Retries=1 -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15)

if sudo apt-get "${opts[@]}" update -qq &&
   sudo apt-get "${opts[@]}" install -y -qq --no-install-recommends "$@"; then
  exit 0
fi

echo "::warning::Ubuntu's archive is unreachable from this runner — installing $* from the OCF mirror (still Ubuntu-signed)"

. /etc/os-release
case "$(dpkg --print-architecture)" in
  amd64 | i386) path=ubuntu ;;
  *) path=ubuntu-ports ;;
esac
base="http://mirrors.ocf.berkeley.edu/$path"

list=$(mktemp)
parts=$(mktemp -d) # empty: ignore the runner's own sources.list.d
for suite in "$VERSION_CODENAME" "$VERSION_CODENAME-updates" "$VERSION_CODENAME-security"; do
  echo "deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] $base $suite main universe"
done >"$list"

fallback=("${opts[@]}" -o "Dir::Etc::SourceList=$list" -o "Dir::Etc::SourceParts=$parts")
sudo apt-get "${fallback[@]}" update -qq
sudo apt-get "${fallback[@]}" install -y -qq --no-install-recommends "$@"
