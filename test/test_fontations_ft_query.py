#! /usr/bin/env python3
# Copyright (C) 2025 Google LLC.
# SPDX-License-Identifier: HPND

from fctest import (
    FcTest,
    FcExternalTestFont,
    FcBrokenFont,
    FcType1TestFont,
    pytest_generate_tests,
)
from pathlib import Path
from enum import Enum
import pytest


class RetCodeBehavior(Enum):
    MUST_BE_ZERO = 1
    MUST_MATCH = 2


@pytest.fixture
def fctest():
    return FcTest()


def compare_fontations_freetype(fctest, font_file, ret_code_behavior: RetCodeBehavior):
    font_path = Path(font_file)

    if not font_path.exists():
        # Skip if file missing
        pytest.skip(f"Font file not found: {font_file}")

    for ret_ft, stdout_ft, stderr_ft in fctest.run_query([font_file]):
        if ret_code_behavior == RetCodeBehavior.MUST_BE_ZERO:
            assert ret_ft == 0, stderr_ft
        result_freetype = stdout_ft.strip().splitlines()
    fctest.with_fontations = True
    for ret_fontations, stdout_fontations, stderr_fontations in fctest.run_query([font_file]):
        if ret_code_behavior == RetCodeBehavior.MUST_BE_ZERO:
            assert ret_fontations == 0, stderr_fontations
        result_fontations = stdout_fontations.strip().splitlines()

    if ret_code_behavior == RetCodeBehavior.MUST_MATCH:
        assert ret_ft == ret_fontations, (
            f"Return codes must match. "
            f"Fontations: {ret_fontations} (stderr: {stderr_fontations}), "
            f"FreeType: {ret_ft} (stderr: {stderr_ft})"
        )

    assert (
        result_freetype == result_fontations
    ), f"FreeType and Fontations fc-query result must match. Fontations: {result_fontations}, FreeType: {result_freetype}"

    return result_fontations


def test_fontations_freetype_fcquery_equal(fctest, parametrized_external_font):
    fctest.logger.info(f'Testing with: {parametrized_external_font}')
    compare_fontations_freetype(
        fctest, parametrized_external_font, RetCodeBehavior.MUST_BE_ZERO)


@pytest.mark.parametrize("font_file", FcBrokenFont().fonts)
def test_fontations_freetype_fcquery_equal_broken_fonts(fctest, font_file):
    fctest.logger.info(
        f'Testing for FreeType equivalence with intentionally broken font: {font_file}')
    compare_fontations_freetype(fctest, font_file, RetCodeBehavior.MUST_MATCH)


type1_fonts = FcType1TestFont().fonts
@pytest.mark.parametrize(
    "font_file",
    type1_fonts
    if type1_fonts
    else [
        pytest.param(
            None,
            marks=pytest.mark.skip(reason="No Type 1 test fonts found"),
        )
    ],
)
def test_fontations_freetype_fcquery_equal_type1(fctest, font_file):
    if not font_file:
        pytest.skip("No Type 1 test fonts found")
    fctest.logger.info(
        f'Testing for FreeType equivalence with Type 1 font: {font_file}')
    compare_fontations_freetype(fctest, font_file, RetCodeBehavior.MUST_BE_ZERO)


def test_fontations_freetype_fcquery_equal_woff2(fctest, parametrized_external_font):
    config_h = Path(fctest.builddir) / "meson-config.h"
    if not config_h.exists():
        config_h = Path(fctest.builddir) / "config.h"
    if config_h.exists() and "#define HAVE_LIBWOFF2DEC 1" not in config_h.read_text():
        pytest.skip("WOFF2 decoding not enabled in build")

    woff2_path = Path(parametrized_external_font).with_suffix(".woff2")
    if not woff2_path.exists():
        pytest.fail(f"WOFF2 font not found: {woff2_path}")

    fctest.logger.info(f"Testing WOFF2 font equivalence: {woff2_path}")
    result_woff2 = compare_fontations_freetype(
        fctest, str(woff2_path), RetCodeBehavior.MUST_BE_ZERO
    )

    # Compare metadata between decoded WOFF2 font and the uncompressed original font
    fctest.with_fontations = False
    for _, stdout_ttf, _ in fctest.run_query([parametrized_external_font]):
        result_ttf = stdout_ttf.strip().splitlines()

    wrapper_woff2 = [l.strip() for l in result_woff2 if l.strip().startswith("fontwrapper:")]
    wrapper_ttf = [l.strip() for l in result_ttf if l.strip().startswith("fontwrapper:")]
    assert len(wrapper_woff2) > 0 and all(w == 'fontwrapper: "WOFF2"(s)' for w in wrapper_woff2), (
        f"Expected WOFF2 fontwrapper for {woff2_path}, got: {wrapper_woff2}"
    )
    assert len(wrapper_ttf) == len(wrapper_woff2) and all(t == 'fontwrapper: "SFNT"(s)' for t in wrapper_ttf), (
        f"Expected SFNT fontwrapper for {parametrized_external_font}, got: {wrapper_ttf}"
    )

    filter_meta = lambda lines: [
        l for l in lines if not l.strip().startswith(("file:", "fontwrapper:"))
    ]
    assert filter_meta(result_woff2) == filter_meta(result_ttf), (
        f"WOFF2 metadata must match uncompressed font metadata for {parametrized_external_font}"
    )
