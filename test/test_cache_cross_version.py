# Copyright (C) 2026 fontconfig Authors
# SPDX-License-Identifier: HPND

"""Cross-version cache format compatibility tests.

Builds a baseline fontconfig from a previous release tag and verifies
that caches written by the current version can be read by the baseline
with identical results.

Old fontconfig accepts cache->version >= FC_CACHE_VERSION_NUMBER, so
the forward direction (new writer → old reader) exercises the real
data path.  The test compares fc-list output between current and
baseline: any difference in font properties indicates the baseline
is misinterpreting the cache data.

The backward direction (old writer → new reader) covers issue #562:
discovery/reuse of an older ``.cache-<N>``, precedence against a stale
shadow cache, and genericfamily backfill at match time.

These tests assume the baseline is within the supported range
(cachemincompat <= cv < current).  The CI driver
(.gitlab-ci/test-cache-compat.sh) selects baselines accordingly; a
baseline below cachemincompat fails loudly rather than silently, and a
cachemincompat that reaches GENERICFAMILY_GUARANTEED_VERSION trips
test_genericfamily_backfill_contract to force the now-dead backfill to
be removed.

Requires FC_BASELINE_BUILDDIR (pre-built baseline) or FC_BASELINE_TAG
(git tag to build from).  Skips if neither is set.
"""

import os
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

import pytest

from fctest import FcTest, FcTestFont


def _read_cache_version_from_meson_build(path):
    """Extract cacheversion integer from a meson.build file."""
    with open(path) as f:
        for line in f:
            m = re.match(r"\s*cacheversion\s*=\s*['\"]?(\d+)", line)
            if m:
                return int(m.group(1))
    return None


def _read_cachemincompat_from_meson_build(path):
    """Extract cachemincompat integer from a meson.build file."""
    with open(path) as f:
        for line in f:
            m = re.match(r"\s*cachemincompat\s*=\s*['\"]?(\d+)", line)
            if m:
                return int(m.group(1))
    return None


# The genericfamily object became a guaranteed part of every cache at
# this cache version: the v10->v11 bump forced regeneration specifically
# to populate it.  Caches older than this lack the object and depend on
# the match-time backfill (FcCompareGenericFamilyBackfill).  Once
# cachemincompat reaches this version, no supported cache can lack it and
# the backfill is dead code -- see test_genericfamily_backfill_contract.
GENERICFAMILY_GUARANTEED_VERSION = 11


def _make_fctest(builddir):
    """Create an FcTest whose binaries come from *builddir*."""
    saved = os.environ.get("builddir")
    os.environ["builddir"] = builddir
    try:
        return FcTest()
    finally:
        if saved is not None:
            os.environ["builddir"] = saved
        else:
            os.environ.pop("builddir", None)


@pytest.fixture(scope="module")
def baseline_builddir(request):
    """Locate or build the baseline fontconfig; return its builddir path."""
    bdir = os.environ.get("FC_BASELINE_BUILDDIR")
    if bdir:
        return bdir

    tag = os.environ.get("FC_BASELINE_TAG")
    if not tag:
        pytest.skip("FC_BASELINE_BUILDDIR or FC_BASELINE_TAG not set")

    srcdir = os.environ.get("srcdir", str(Path(__file__).parents[1]))
    build_py = str(Path(srcdir) / ".gitlab-ci" / "build.py")
    upstream = (
        "https://gitlab.freedesktop.org/"
        f"{os.environ.get('FDO_UPSTREAM_REPO', 'fontconfig/fontconfig')}.git"
    )

    tmpgit_td = TemporaryDirectory(prefix="fc-baseline-git.")
    request.addfinalizer(tmpgit_td.cleanup)
    subprocess.run(
        ["git", "init", "--bare", tmpgit_td.name],
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "-C", tmpgit_td.name, "fetch", "--depth=1",
         upstream, "tag", tag],
        check=True,
    )

    src_td = TemporaryDirectory(prefix="fc-baseline-src.")
    request.addfinalizer(src_td.cleanup)
    archive = subprocess.Popen(
        ["git", "-C", tmpgit_td.name, "archive", tag],
        stdout=subprocess.PIPE,
    )
    subprocess.run(
        ["tar", "-C", src_td.name, "-x"],
        stdin=archive.stdout, check=True,
    )
    archive.wait()
    assert archive.returncode == 0, f"git archive {tag} failed"

    bld = str(Path(src_td.name) / "build")
    pfx = str(Path(src_td.name) / "prefix")
    env = os.environ.copy()
    env["BUILDDIR"] = bld
    env["PREFIX"] = pfx
    subprocess.run(
        [sys.executable, build_py, "--source-dir", src_td.name,
         "-C", "-I", "-V", "-d", "nls", "-d", "doc"],
        env=env, check=True,
    )
    return bld


@pytest.fixture(scope="module")
def baseline_cache_version(baseline_builddir):
    """Cache version of the baseline build."""
    cv_str = os.environ.get("FC_BASELINE_CACHE_VERSION")
    if cv_str:
        return int(cv_str)
    src = Path(baseline_builddir).parent
    cv = _read_cache_version_from_meson_build(src / "meson.build")
    if cv is not None:
        return cv
    pytest.fail("Cannot determine baseline cache version")


@pytest.fixture
def current_cache_version():
    """Cache version of the current build."""
    cv = os.environ.get("FC_CACHE_VERSION")
    if cv:
        return int(cv)
    srcdir = os.environ.get("srcdir", str(Path(__file__).parents[1]))
    return _read_cache_version_from_meson_build(Path(srcdir) / "meson.build")


@pytest.fixture
def current_cache_mincompat():
    """Oldest cache version the current reader promises to accept."""
    mc = os.environ.get("FC_CACHE_MINCOMPAT")
    if mc:
        return int(mc)
    srcdir = os.environ.get("srcdir", str(Path(__file__).parents[1]))
    mc = _read_cachemincompat_from_meson_build(Path(srcdir) / "meson.build")
    assert mc is not None, "cannot determine cachemincompat from meson.build"
    return mc


def _require_supported_baseline(old_cv, mincompat):
    """Fail loudly when the baseline predates the supported range.

    A format break that raises cachemincompat is *expected* to make the
    backward-compat reuse scenario invalid -- the reader now rejects such
    caches on purpose.  Turn that into a clear, intentional failure that
    names what to revisit, rather than a confusing low-level assertion
    (e.g. "cache was regenerated") that looks like a real regression.
    """
    if old_cv < mincompat:
        pytest.fail(
            f"baseline cache version {old_cv} is below cachemincompat="
            f"{mincompat}: reading such caches is no longer part of the "
            "contract, so these reuse tests cannot pass. If a format break "
            "raised cachemincompat, update the baseline selection in "
            ".gitlab-ci/test-cache-compat.sh to target the new boundary and "
            "review whether the backward-compat tests should now assert "
            "rejection instead of reuse."
        )


@pytest.fixture
def fctest():
    return FcTest()


@pytest.fixture
def fcfont():
    return FcTestFont()


def _cache_suffix(version):
    return f".cache-{version}"


def _cache_files_with_suffix(cachedir, suffix):
    return list(Path(cachedir).glob(f"*{suffix}"))


def _symlink_caches(cachedir, from_suffix, to_suffix):
    """Create symlinks: for each *from_suffix* file, link a *to_suffix* name."""
    for f in _cache_files_with_suffix(cachedir, from_suffix):
        target = f.parent / f.name.replace(from_suffix, to_suffix)
        if not target.exists():
            target.symlink_to(f.name)


def _snapshot_cache_inodes(cachedir):
    """Return a dict mapping cache filename → (inode, mtime_ns)."""
    return {
        f.name: (f.stat().st_ino, f.stat().st_mtime_ns)
        for f in Path(cachedir).glob("*cache*")
    }


# FcCache on-disk header begins with: unsigned int magic; int version; ...
_FC_CACHE_MAGIC_MMAP = 0xFC02FC04


def _read_cache_version_field(path):
    """Read the ``version`` field written into a real .cache-* file.

    Guards against baseline binaries silently loading the wrong
    libfontconfig: a v9 baseline must actually stamp version == 9 on
    disk, otherwise a same-version contamination would make the
    backward-compat test meaningless.
    """
    with open(path, "rb") as f:
        header = f.read(8)
    magic, version = struct.unpack("<Ii", header)
    assert magic == _FC_CACHE_MAGIC_MMAP, (
        f"{path}: not an mmap cache (magic={magic:#x})"
    )
    return version


def _parse_verbose_output(text):
    """Parse fc-list -v output into a dict keyed by file path.

    Each pattern becomes a dict mapping property names to their
    raw value strings, keyed by the ``file`` property so patterns
    from different versions can be matched regardless of order.
    """
    patterns = {}
    current = {}
    for line in text.splitlines():
        if line.startswith("Pattern has "):
            if current and "file" in current:
                patterns[current["file"]] = current
            current = {}
        elif line.startswith("\t") and ":" in line:
            key, _, val = line.strip().partition(":")
            current[key.strip()] = val.strip()
    if current and "file" in current:
        patterns[current["file"]] = current
    return patterns


# Properties whose values legitimately differ across fontconfig
# versions even when the cache format is compatible.
_SKIP_PROPERTIES = {
    "lang",
}


def _setup_and_share(fctest, fcfont, baseline_builddir):
    """Set up fonts/config via fctest, return a baseline FcTest sharing it."""
    fctest.setup()
    fctest.install_font(fcfont.fonts, ".")
    baseline = _make_fctest(baseline_builddir)
    baseline._env["FONTCONFIG_FILE"] = fctest._env["FONTCONFIG_FILE"]
    return baseline


# ---- CI-gating test: forward binary format compatibility ----


@pytest.mark.skipif(os.getenv("EXEEXT", "") != "", reason="not working on Win32")
def test_binary_format_forward_compat(
    baseline_builddir, baseline_cache_version, current_cache_version,
    fctest, fcfont,
):
    """Cache written by current fc-cache must be readable by baseline fc-list.

    Compares verbose fc-list output between current and baseline.
    Properties that both versions share must produce identical values;
    any divergence means the baseline is misinterpreting the cache.
    """
    baseline = _setup_and_share(fctest, fcfont, baseline_builddir)
    cachedir = fctest.cachedir.name
    old_cv = baseline_cache_version
    new_cv = current_cache_version

    for ret, _, stderr in fctest.run_cache([fctest.fontdir.name]):
        assert ret == 0, f"Current fc-cache failed: {stderr}"

    new_suffix = _cache_suffix(new_cv)
    assert _cache_files_with_suffix(cachedir, new_suffix), (
        f"No {new_suffix} cache files generated"
    )

    for ret, current_out, stderr in fctest.run_list(["-v"]):
        assert ret == 0, f"Current fc-list failed: {stderr}"

    if old_cv != new_cv:
        _symlink_caches(cachedir, new_suffix, _cache_suffix(old_cv))

    snap = _snapshot_cache_inodes(cachedir)

    for ret, baseline_out, stderr in baseline.run_list(["-v"]):
        assert ret == 0, f"Baseline fc-list failed: {stderr}"
        assert baseline_out.strip(), "Baseline fc-list returned no output"

    snap_after = _snapshot_cache_inodes(cachedir)
    assert snap == snap_after, (
        "Baseline fc-list regenerated cache instead of using current's"
    )

    current_patterns = _parse_verbose_output(current_out)
    baseline_patterns = _parse_verbose_output(baseline_out)

    assert len(baseline_patterns) == len(current_patterns), (
        f"Font count mismatch: baseline saw {len(baseline_patterns)}, "
        f"current saw {len(current_patterns)}"
    )

    for file_key, cur in current_patterns.items():
        assert file_key in baseline_patterns, (
            f"Baseline did not find font {file_key}"
        )
        base = baseline_patterns[file_key]
        shared_keys = (
            set(cur.keys()) & set(base.keys()) - _SKIP_PROPERTIES
        )
        for key in sorted(shared_keys):
            assert base[key] == cur[key], (
                f"Font {cur.get('family', '?')}, property '{key}': "
                f"baseline={base[key]!r}, current={cur[key]!r}"
            )


# ---- Discovery tests: xfail until symlinks/probing implemented ----


@pytest.mark.skipif(os.getenv("EXEEXT", "") != "", reason="not working on Win32")
def test_discovery_backward_compat(
    baseline_builddir, baseline_cache_version, current_cache_version,
    current_cache_mincompat, fctest, fcfont,
):
    """Old cache written by baseline must be reused by current fc-list.

    This is the issue #562 repro (Silverblue host v9 cache read by a
    v12 sandbox reader).  The baseline (old) fontconfig writes a
    ``.cache-<old_cv>`` file; the current fontconfig must then discover
    and REUSE it rather than regenerating a ``.cache-<new_cv>``.

    The discriminator is cache REUSE (inode/mtime snapshot), not font
    visibility: current fontconfig rescans in-memory when it rejects a
    cache, so fonts are listed either way -- only the snapshot reveals
    whether the old cache was actually read.
    """
    old_cv = baseline_cache_version
    new_cv = current_cache_version
    _require_supported_baseline(old_cv, current_cache_mincompat)

    baseline = _setup_and_share(fctest, fcfont, baseline_builddir)
    cachedir = fctest.cachedir.name

    for ret, _, stderr in baseline.run_cache([fctest.fontdir.name]):
        assert ret == 0, f"Baseline fc-cache failed: {stderr}"

    # Guard against same-version contamination: the baseline must have
    # actually stamped its own version on disk, otherwise "backward"
    # compat is not being exercised at all.
    old_caches = _cache_files_with_suffix(cachedir, _cache_suffix(old_cv))
    old_caches = [c for c in old_caches if not c.is_symlink()]
    assert old_caches, f"Baseline did not write a .cache-{old_cv} file"
    for c in old_caches:
        assert _read_cache_version_field(c) == old_cv, (
            f"{c.name}: on-disk version != baseline cache version "
            f"{old_cv} (baseline loaded the wrong libfontconfig?)"
        )

    snap = _snapshot_cache_inodes(cachedir)

    for ret, stdout, stderr in fctest.run_list(["-", "family"]):
        assert ret == 0, f"Current fc-list failed: {stderr}"
        assert stdout.strip(), "fc-list returned no output"

    snap_after = _snapshot_cache_inodes(cachedir)
    assert snap == snap_after, (
        "Current fc-list regenerated cache instead of reusing the "
        f"baseline's .cache-{old_cv} -- backward-compat discovery failed"
    )


@pytest.mark.skipif(os.getenv("EXEEXT", "") != "", reason="not working on Win32")
def test_shadow_cache_precedence_562(
    request, baseline_builddir, baseline_cache_version, current_cache_version,
    current_cache_mincompat, fcfont,
):
    """Faithful issue #562 repro: a stale shadow cache hides host fonts.

    Reproduces the Silverblue-inside-Flatpak layout that makes
    /run/host/fonts vanish:

      * the queried dir D carries mtime 0 (OSTree), so
        FcCacheTimeValid short-circuits and trusts any same-key cache
        without checksum comparison;
      * a SHADOW cachedir searched FIRST holds a current-version cache
        for D built from *different* content (mtime 0) -- standing in
        for the runtime's stale ``/usr/lib/.../<key>.cache-<new>``;
      * a HOST cachedir searched LAST holds the real cache for D
        written by the OLD baseline as ``.cache-<old>`` (mtime 0) --
        standing in for ``/run/host/fonts-cache``.

    Because both caches share one MD5 key (D's path is identical) and
    D has mtime 0, the OSTree branch of the cache selector prefers the
    last-enumerated zero-mtime cache: the host cache -- *provided the
    reader can read it*.  Before the read-side backward-compat fix, the
    current reader rejects the old-version host cache, is left with
    only the shadow, and the host font disappears -- exactly #562.

    This test mechanically settles the open ordering caveat: it passes
    only if backward reading is sufficient for the current cachedir
    ordering, and keeps failing if the shadow still wins.
    """
    old_cv = baseline_cache_version
    new_cv = current_cache_version
    if old_cv == new_cv:
        pytest.skip(
            "baseline shares the current cache version; "
            "no cross-version shadow scenario to reproduce"
        )
    _require_supported_baseline(old_cv, current_cache_mincompat)

    shadow_font = Path(fcfont.fonts[0])   # 4x6.pcf  -> shadow (wrong) content
    host_font = Path(fcfont.fonts[1])     # 8x16.pcf -> real host content

    td = TemporaryDirectory(prefix="fc562.")
    request.addfinalizer(td.cleanup)
    base = Path(td.name)
    fontdir = base / "fonts"        # queried dir D (== /run/host/fonts)
    shadow_cd = base / "shadow"     # searched first (== /usr/lib/.../cache)
    host_cd = base / "host"         # searched last  (== /run/host/fonts-cache)
    for d in (fontdir, shadow_cd, host_cd):
        d.mkdir()

    cur = FcTest()
    baseline = _make_fctest(baseline_builddir)

    def write_conf(name, cachedirs):
        p = base / name
        cds = "".join(f"<cachedir>{c}</cachedir>" for c in cachedirs)
        p.write_text(f"<fontconfig><dir>{fontdir}</dir>{cds}</fontconfig>")
        return str(p)

    # 1. SHADOW cache: current version, wrong content.
    shutil.copy(shadow_font, fontdir / shadow_font.name)
    cur._env["FONTCONFIG_FILE"] = write_conf("shadow.conf", [shadow_cd])
    for ret, _, stderr in cur.run_cache([str(fontdir)]):
        assert ret == 0, f"current fc-cache (shadow) failed: {stderr}"
    (fontdir / shadow_font.name).unlink()

    shadow_caches = _cache_files_with_suffix(shadow_cd, _cache_suffix(new_cv))
    shadow_caches = [c for c in shadow_caches if not c.is_symlink()]
    assert shadow_caches, f"no shadow .cache-{new_cv} written"

    # 2. HOST cache: old baseline version, real content.
    shutil.copy(host_font, fontdir / host_font.name)
    baseline._env["FONTCONFIG_FILE"] = write_conf("host.conf", [host_cd])
    for ret, _, stderr in baseline.run_cache([str(fontdir)]):
        assert ret == 0, f"baseline fc-cache (host) failed: {stderr}"

    host_caches = [
        c for c in _cache_files_with_suffix(host_cd, _cache_suffix(old_cv))
        if not c.is_symlink()
    ]
    assert host_caches, f"no host .cache-{old_cv} written"
    for c in host_caches:
        assert _read_cache_version_field(c) == old_cv, (
            f"{c.name}: baseline wrote version != {old_cv} "
            f"(baseline loaded the wrong libfontconfig?)"
        )

    # Both caches must share one MD5 key (same dir path, differing only
    # by suffix); otherwise there is no shadowing to reproduce.
    shadow_key = shadow_caches[0].name.rsplit(".cache-", 1)[0]
    host_key = host_caches[0].name.rsplit(".cache-", 1)[0]
    assert shadow_key == host_key, (
        f"cache keys differ ({shadow_key} vs {host_key}); no collision"
    )

    # 3. OSTree layout: dir and both caches carry mtime 0.
    for p in [fontdir, *shadow_cd.glob("*"), *host_cd.glob("*")]:
        os.utime(p, (0, 0))

    # 4. Query with the current tools; shadow first, host last.
    cur._env["FONTCONFIG_FILE"] = write_conf("query.conf", [shadow_cd, host_cd])
    snap = _snapshot_cache_inodes(shadow_cd) | _snapshot_cache_inodes(host_cd)
    files = []
    for ret, stdout, stderr in cur.run_list(["-f", "%{file}\n"]):
        assert ret == 0, f"current fc-list failed: {stderr}"
        files = [
            Path(line).name for line in stdout.splitlines() if line.strip()
        ]
    snap_after = (
        _snapshot_cache_inodes(shadow_cd) | _snapshot_cache_inodes(host_cd)
    )

    assert snap == snap_after, (
        "fc-list regenerated a cache; the result would reflect a rescan, "
        "not cache precedence"
    )
    assert host_font.name in files, (
        f"host font {host_font.name} hidden by the shadow cache; "
        f"fc-list saw {files} (issue #562)"
    )


def test_genericfamily_backfill_contract(current_cache_mincompat):
    """Tripwire: retire the genericfamily backfill once no supported cache
    can lack the object.

    The match-time backfill (FcCompareGenericFamilyBackfill in fcmatch.c,
    FcGenericFamilyGetValues in fcgenericalias.c + fcint.h) exists solely
    to serve caches written before genericfamily became guaranteed
    (cache version GENERICFAMILY_GUARANTEED_VERSION).  Once cachemincompat
    reaches that version every supported cache already carries the object,
    the backfill is unreachable dead code, and this assertion fails ON
    PURPOSE to force its removal -- together with
    test_match_equivalence_backward_compat.

    This runs without a baseline, so it guards the invariant in the normal
    test suite, not only in the cross-version CI job.
    """
    assert current_cache_mincompat < GENERICFAMILY_GUARANTEED_VERSION, (
        f"cachemincompat={current_cache_mincompat} >= "
        f"{GENERICFAMILY_GUARANTEED_VERSION}: every supported cache now "
        "carries the genericfamily object, so the match-time backfill is "
        "dead code. Remove FcCompareGenericFamilyBackfill (fcmatch.c), "
        "FcGenericFamilyGetValues (fcgenericalias.c + fcint.h), and "
        "test_match_equivalence_backward_compat."
    )


@pytest.mark.skipif(os.getenv("EXEEXT", "") != "", reason="not working on Win32")
def test_match_equivalence_backward_compat(
    request, baseline_builddir, baseline_cache_version, current_cache_version,
    current_cache_mincompat, fcfont,
):
    """Matching via an old (v9) cache must equal matching via a v12 cache.

    Guards the genericfamily backfill.  Caches older than v11 lack the
    genericfamily object; once the reader is allowed to *use* such a
    cache, a generic query (serif / monospace / ...) mis-scores an
    old-cache font -- the merge-join skips the missing object and leaves
    an unearned perfect (0.0) score -- so the match can diverge from the
    current-cache result.  Deriving genericfamily from the font's family
    at match time (backfill) must make the two identical.

    The 'monospace' query is decisive here: 'Fixed' (the .pcf fonts) is
    a curated monospace family, while no_family_name is unclassifiable,
    so a v9 cache without the backfill mis-ranks them.

    NOTE: this only exercises the regression once the read-side version
    relax is in place -- before that, current fontconfig rejects the v9
    cache and rescans, which recomputes genericfamily and masks the
    difference.
    """
    old_cv = baseline_cache_version
    new_cv = current_cache_version
    if old_cv == new_cv:
        pytest.skip(
            "baseline shares the current cache version; "
            "no cross-version matching to compare"
        )
    _require_supported_baseline(old_cv, current_cache_mincompat)
    if old_cv >= GENERICFAMILY_GUARANTEED_VERSION:
        pytest.skip(
            f"baseline cache v{old_cv} already carries the genericfamily "
            "object; the backfill is only exercised by a pre-v"
            f"{GENERICFAMILY_GUARANTEED_VERSION} baseline (the "
            "cachemincompat-boundary run covers it)"
        )

    srcdir = os.environ.get("srcdir", str(Path(__file__).parents[1]))
    # 'Fixed' (the .pcf fonts) is in the curated monospace list, so it is
    # classified reliably; no_family_name is not classifiable (and has no
    # substring hint), so it is FC_FAMILY_UNKNOWN in both a v12 scan and
    # the backfill.  Together the 'monospace' query is decided purely by
    # the reliable classification path -- no dependence on the scanner's
    # substring guess (which the backfill deliberately omits).
    other = Path(srcdir) / "test" / "no_family_name.ttf"
    assert other.exists(), f"missing test font {other}"
    # genericfamily only participates in matching when the query pattern
    # carries it, which 48-guessfamily.conf populates from the family
    # name.  Without this include the whole dimension is inert and the
    # test would be vacuous.
    guessfamily = Path(srcdir) / "conf.d" / "48-guessfamily.conf"
    assert guessfamily.exists(), f"missing {guessfamily}"

    td = TemporaryDirectory(prefix="fc562match.")
    request.addfinalizer(td.cleanup)
    base = Path(td.name)
    fontdir = base / "fonts"
    cd_old = base / "cache_old"   # v9, written by baseline
    cd_new = base / "cache_new"   # v12, written by current
    for d in (fontdir, cd_old, cd_new):
        d.mkdir()
    for f in list(fcfont.fonts) + [other]:
        shutil.copy(f, fontdir / Path(f).name)

    cur = FcTest()
    baseline = _make_fctest(baseline_builddir)

    def write_conf(name, cachedir):
        p = base / name
        p.write_text(
            f"<fontconfig>"
            f'<include ignore_missing="no">{guessfamily}</include>'
            f"<dir>{fontdir}</dir>"
            f"<cachedir>{cachedir}</cachedir></fontconfig>"
        )
        return str(p)

    # v9 cache from the OLD baseline, v12 cache from the current build.
    baseline._env["FONTCONFIG_FILE"] = write_conf("old.conf", cd_old)
    for ret, _, stderr in baseline.run_cache([str(fontdir)]):
        assert ret == 0, f"baseline fc-cache failed: {stderr}"
    cur._env["FONTCONFIG_FILE"] = write_conf("new.conf", cd_new)
    for ret, _, stderr in cur.run_cache([str(fontdir)]):
        assert ret == 0, f"current fc-cache failed: {stderr}"

    old_caches = [
        c for c in _cache_files_with_suffix(cd_old, _cache_suffix(old_cv))
        if not c.is_symlink()
    ]
    assert old_caches, f"baseline wrote no .cache-{old_cv}"
    assert _read_cache_version_field(old_caches[0]) == old_cv

    def match(cachedir, query):
        conf = write_conf(f"q_{Path(cachedir).name}.conf", cachedir)
        cur._env["FONTCONFIG_FILE"] = conf
        out = None
        for ret, stdout, stderr in cur.run_match(["-f", "%{file}", query]):
            assert ret == 0, f"fc-match {query!r} failed: {stderr}"
            out = Path(stdout.strip()).name if stdout.strip() else ""
        return out

    # Compare current-via-v9 against current-via-v12 for generic queries
    # where genericfamily is the deciding factor.
    diffs = {}
    for query in ("serif", "monospace", "sans-serif"):
        via_old = match(cd_old, query)
        via_new = match(cd_new, query)
        if via_old != via_new:
            diffs[query] = (via_old, via_new)

    assert not diffs, (
        "matching via v9 cache differs from v12 cache "
        f"(genericfamily lost): {diffs}"
    )


@pytest.mark.skipif(os.getenv("EXEEXT", "") != "", reason="not working on Win32")
def test_discovery_forward_compat(
    baseline_builddir, baseline_cache_version, current_cache_version,
    fctest, fcfont,
):
    """New cache must be discoverable by old fc-list without renaming.

    Current fc-cache creates compat symlinks (.cache-9, .cache-10, ...)
    pointing to the real cache file, so old fontconfig can find them.
    """
    baseline = _setup_and_share(fctest, fcfont, baseline_builddir)
    cachedir = fctest.cachedir.name

    for ret, _, stderr in fctest.run_cache([fctest.fontdir.name]):
        assert ret == 0, f"Current fc-cache failed: {stderr}"

    old_cv = baseline_cache_version
    new_cv = current_cache_version
    if old_cv != new_cv:
        old_suffix = _cache_suffix(old_cv)
        symlinks = _cache_files_with_suffix(cachedir, old_suffix)
        assert symlinks, (
            f"fc-cache did not create compat symlinks for .cache-{old_cv}"
        )
        for s in symlinks:
            assert s.is_symlink(), (
                f"{s.name} should be a symlink, not a regular file"
            )

    snap = _snapshot_cache_inodes(cachedir)

    for ret, stdout, stderr in baseline.run_list(["-", "family"]):
        assert ret == 0, f"Baseline fc-list failed: {stderr}"
        assert stdout.strip(), "Baseline fc-list returned no output"

    snap_after = _snapshot_cache_inodes(cachedir)
    assert snap == snap_after, (
        "Baseline fc-list regenerated cache -- discovery failed"
    )
