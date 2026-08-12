#!/usr/bin/env python3
"""Shared statistical variation policy for release performance evidence."""

from __future__ import annotations

from typing import Literal


NORMAL_VARIATION_MAX = 0.10
WARNING_VARIATION_MAX = 0.20
MIN_RELEASE_SAMPLES = 5

VariationClass = Literal["normal", "warning", "hard_failure"]


def classify_relative_mad(relative_mad: float) -> VariationClass:
    if relative_mad > WARNING_VARIATION_MAX:
        return "hard_failure"
    if relative_mad > NORMAL_VARIATION_MAX:
        return "warning"
    return "normal"
