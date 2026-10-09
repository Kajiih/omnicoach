"""Exemption mutation specs, one module per audit cluster (see `scripts/exemption_mutations.py`)."""

from __future__ import annotations

from . import declaration_order, literals_and_placeholders

CLUSTERS = (
    declaration_order.CLUSTER,
    literals_and_placeholders.CLUSTER,
)
