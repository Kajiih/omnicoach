# Phase 7: Learn — `unmatched-logger-placeholder` (`LoggerPositionalPlaceholderRule`)

This document captures the reusable lessons from designing, implementing, and auditing `unmatched-logger-placeholder` (`LoggerPositionalPlaceholderRule`).

> Status: **COMPLETE.**

---

## 1. Key Takeaways

1. **Complement Rather Than Duplicate Ruff's `flake8-logging-format` (`G`) Rules**:
   - Auditing Ruff's `G` and `PLE1205`/`PLE1206` rules in Phase 2 revealed a concrete ecosystem gap: Ruff flags `logger.info("User {}", user)` when positional arguments are passed (stdlib `logging` raises `TypeError: not all arguments converted during string formatting`), and flags `logger.info(f"User {user}")` (`G004`), but fixing `G004` by dropping the `f` and passing `user` positionally yields `logger.info("User {user}", user)`. That call raises `KeyError` in Loguru, yet it passes Ruff in Loguru projects, which must disable `PLE1205`. The rule flags that call and never flags calls without positional format arguments, such as `logger.info("User {}")`.
2. **Account for `log(level, msg, *args)` Argument Offset and Count Only Positional Format Arguments**:
   - Standard logger methods (`debug`, `info`, `warning`, `warn`, `error`, `exception`, `critical`, `fatal`) take `msg` at positional index `0`, whereas `.log(level, msg, *args)` takes `msg` at positional index `1` (or keyword `msg=`). `check_logger_call_node` counts only positional arguments after the message as format arguments. Keyword arguments never count, whether stdlib ones (`exc_info`, `stack_info`, `stacklevel`, `extra`) or Loguru format arguments. Their names serve only to satisfy matching named placeholders. So `logger.info("User {id}", extra={"id": 1})` and Loguru's `logger.info("User {id}", id=1)` both have zero format arguments and are not flagged.
3. **Self-Dogfooding Catches Variable Naming Violations in Rust AST Code**:
   - Running `cargo run --bin omni-code-lint -- .` during Phase 4 caught `let Some(argument_list) = ...` in `src/code_lint/ast/python.rs` under `type-suffixed-name` (`_list` suffix). Renaming to `arguments` kept the repository 100% self-dogfooding clean without needing suppression comments.
