#!/bin/bash
# Copyright (C) 2026 fontconfig Authors
# SPDX-License-Identifier: HPND
#
# Cache compatibility test: builds one or more baseline fontconfig
# releases and runs the cross-version cache read/write tests against each.
#
# Baseline selection (when FC_BASELINE_TAG is empty) deliberately targets
# the *contract boundary* rather than just the nearest older release:
#
#   * boundary baseline -- the newest release at the OLDEST still-supported
#     cache version (>= cachemincompat).  This is the hardest case and the
#     one that reproduces issue #562 and exercises the genericfamily
#     backfill; it also tracks the documented cachemincompat contract, so
#     the coverage follows the code automatically when cachemincompat moves.
#   * canary baseline -- the newest release still older than current (the
#     smallest version jump), as a cheap "did we just break the most recent
#     format" check.
#
# Both are filtered to [cachemincompat, current): a release below
# cachemincompat is no longer readable by design and would only produce a
# false failure.
#
# Environment:
#   FC_BASELINE_TAG  - Pin a specific baseline tag (e.g. "2.17.1").
#                      If empty, auto-detects as described above.
#   FDO_UPSTREAM_REPO - Upstream repo path (default: fontconfig/fontconfig)

set -e

upstream="https://gitlab.freedesktop.org/${FDO_UPSTREAM_REPO:-fontconfig/fontconfig}.git"

# Current build directory from parent artifacts
builddir=$(echo "$(pwd)"/build-fontconfig-*)

# Read current cache version and minimum compatible version from the
# source meson.build (not the generated header).
current_cv=$(grep -m1 'cacheversion\s*=' meson.build | tr -dc '0-9')
mincompat=$(grep -m1 'cachemincompat\s*=' meson.build | tr -dc '0-9')
echo ">>> Current cache version: $current_cv (min compatible: $mincompat)"

# Temporary bare repo for all git operations — avoids polluting
# the source tree with fetched tags/objects (important for local runs)
tmpgit=$(mktemp -d)
baseline_srcs=""
trap 'rm -rf "$tmpgit" $baseline_srcs' EXIT
git init --bare "$tmpgit" >/dev/null 2>&1

# Fetch a tag into the temp repo and echo its cache version (empty if the
# tag has no meson.build / cacheversion).  Returns non-zero if the tag
# cannot be fetched.
tag_cache_version() {
    local t=$1
    git -C "$tmpgit" fetch --depth=1 "$upstream" tag "$t" >/dev/null 2>&1 || return 1
    git -C "$tmpgit" show "$t:meson.build" 2>/dev/null \
        | grep -m1 'cacheversion\s*=' | tr -dc '0-9'
}

# --- Baseline tag selection ---
baseline_tags=()
if [ -n "$FC_BASELINE_TAG" ]; then
    echo ">>> Using pinned baseline: $FC_BASELINE_TAG"
    baseline_tags=("$FC_BASELINE_TAG")
else
    echo ">>> Auto-detecting baseline tags in [$mincompat, $current_cv)..."
    declare -A cv_newest_tag
    # Tags are visited newest-first, so the first tag seen at a given
    # cache version is the newest release at that version.
    for tag in $(git ls-remote --tags --refs "$upstream" '2.*' \
                 | awk '{print $2}' | sed 's|refs/tags/||' | sort -Vr); do
        cv=$(tag_cache_version "$tag") || continue
        [ -z "$cv" ] && continue
        [ "$cv" -ge "$current_cv" ] && continue
        # Older than the supported range: everything further back is too
        # old as well (versions descend), so stop scanning.
        [ "$cv" -lt "$mincompat" ] && break
        [ -z "${cv_newest_tag[$cv]}" ] && cv_newest_tag[$cv]="$tag"
        # Reached the oldest supportable version; no smaller in-range cv
        # can exist, so stop (bounds the number of tag fetches).
        [ "$cv" -eq "$mincompat" ] && break
    done

    if [ ${#cv_newest_tag[@]} -eq 0 ]; then
        echo ">>> No supported older cache version found among release tags, skipping"
        exit 0
    fi

    sorted_cvs=$(for k in "${!cv_newest_tag[@]}"; do echo "$k"; done | sort -n)
    boundary_cv=$(echo "$sorted_cvs" | head -1)
    canary_cv=$(echo "$sorted_cvs" | tail -1)

    boundary_tag="${cv_newest_tag[$boundary_cv]}"
    baseline_tags=("$boundary_tag")
    echo ">>> cachemincompat-boundary baseline: $boundary_tag (cache version $boundary_cv)"
    if [ "$canary_cv" != "$boundary_cv" ]; then
        canary_tag="${cv_newest_tag[$canary_cv]}"
        baseline_tags+=("$canary_tag")
        echo ">>> newest-older canary baseline: $canary_tag (cache version $canary_cv)"
    fi
fi

# Build a baseline from its source and run the cross-version tests.
run_against_baseline() {
    local tag=$1 tcv=$2
    local baseline_src
    baseline_src=$(mktemp -d)
    baseline_srcs="$baseline_srcs $baseline_src"

    git -C "$tmpgit" archive "$tag" | tar -C "$baseline_src" -x
    if [ -z "$tcv" ]; then
        tcv=$(grep -m1 'cacheversion\s*=' "$baseline_src/meson.build" | tr -dc '0-9')
    fi
    echo ">>> Building baseline $tag (cache version $tcv)..."
    BUILDDIR="$baseline_src/build" \
    PREFIX="$baseline_src/prefix" \
        python3 .gitlab-ci/build.py \
            --source-dir "$baseline_src" \
            -C -I \
            -d nls -d doc

    # Activate venv (build.py creates it if missing; provides pytest, etc.)
    if [ -f .venv/bin/activate ]; then
        . .venv/bin/activate
    fi

    echo ">>> Running cache compatibility tests against $tag..."
    (
        cd test
        FC_BASELINE_BUILDDIR="$baseline_src/build" \
        FC_BASELINE_CACHE_VERSION="$tcv" \
        builddir="$builddir" \
        srcdir="$(pwd)/.." \
            python3 -m pytest -v test_cache_cross_version.py \
                --tap --assert=plain
    )
}

for tag in "${baseline_tags[@]}"; do
    # Ensure the tag is fetched and learn its cache version (idempotent
    # for auto-detected tags already fetched above).
    tcv=$(tag_cache_version "$tag") || {
        echo ">>> Failed to fetch baseline tag $tag" >&2
        exit 1
    }
    run_against_baseline "$tag" "$tcv"
done
