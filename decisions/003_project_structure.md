# Project Structure & Naming

## Decision: Single Crate Package Architecture (Consolidated from Workspace)

To ensure the toolkit is **standalone, modular, and not tied to any specific downstream environment**, we structure the codebase as a single Cargo package (`omni`) with domain-specific library modules and binary targets. While we initially designed the system around a **Cargo Workspace**, we consolidated it into a single crate to reduce dependency and build overhead while maintaining strict logical boundaries via modules.

We use the name **Omni** for the crate, reflecting the tool's ability to check everything (code, commands, and workflow context). Component boundaries and module layout are specified and enforced by the architectural DAG in [ADR 006](006_architectural_dag_and_conformance.md).
