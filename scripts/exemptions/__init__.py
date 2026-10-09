"""Exemption mutation specs, one module per audit cluster (see `scripts/exemption_mutations.py`)."""

from __future__ import annotations

from . import (
    collection_types_1,
    collection_types_2,
    declaration_order,
    literals_and_placeholders,
    names,
    tests_cluster,
)

CLUSTERS = (
    declaration_order.CLUSTER,
    literals_and_placeholders.CLUSTER,
    collection_types_1.CLUSTER,
    collection_types_2.CLUSTER,
    names.CLUSTER,
    tests_cluster.CLUSTER,
)
