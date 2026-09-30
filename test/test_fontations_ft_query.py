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

