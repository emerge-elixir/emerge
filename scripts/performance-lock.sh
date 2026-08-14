#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 [--source-revision immutable-id] shared|exclusive command [args...]" >&2
  exit 64
}

source_revision=${EMERGE_SOURCE_REVISION:-}
if [[ ${1:-} == "--source-revision" ]]; then
  [[ $# -ge 4 && -n ${2:-} ]] || usage
  source_revision=$2
  shift 2
fi

[[ $# -ge 2 ]] || usage
mode=$1
shift

case "$mode" in
  shared) lock_flag=-s ;;
  exclusive) lock_flag=-x ;;
  *) usage ;;
esac

if [[ -z $source_revision ]] && git_root=$(git rev-parse --show-toplevel 2>/dev/null); then
  head_revision=$(git -C "$git_root" rev-parse HEAD)
  if [[ -n $(git -C "$git_root" status --porcelain --untracked-files=normal) ]]; then
    source_revision="unknown-dirty@${head_revision}"
    echo "MEASUREMENT IDENTITY WARNING: dirty worktree has no immutable source id; pass --source-revision <immutable-id> for measurement builds." >&2
  else
    source_revision=$head_revision
  fi
fi
if [[ -z $source_revision ]]; then
  source_revision=unknown
  echo "MEASUREMENT IDENTITY WARNING: source revision is unknown; pass --source-revision <immutable-id>." >&2
fi
export EMERGE_SOURCE_REVISION=$source_revision

lock_path=${EMERGE_PERFORMANCE_LOCK:-/tmp/emerge-performance.lock}
exec flock "$lock_flag" "$lock_path" "$@"
