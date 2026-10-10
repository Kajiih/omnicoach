# How Omni resolves names

Some rules match specific functions, macros, decorators, or types: `sleep-in-tests` flags `time.sleep`, `type-cast` flags `typing.cast`, and the collection-type rules recognize `list` or `typing.Dict` in annotations. These rules match what a name refers to in the file, not only how it is spelled. This page describes that matching once, so the rule docs don't repeat it.

## Matching

Omni looks at the first segment of a name (`t` in `t.cast`, `thread` in `thread::sleep`):

- **Imported**: the name is rewritten to its full path, and only that path matches. After `import typing as t`, `t.cast(...)` is `typing.cast`. After `from unittest.mock import patch as p`, `p(...)` is `unittest.mock.patch`. After `use std::thread;`, `thread::sleep(...)` is `std::thread::sleep`. A `cast` imported from another library, such as `from sqlalchemy import cast`, does not match `typing.cast` or a bare `cast` entry.
- **Defined in the file**: a function, class or Rust item with that name never matches, so a local `def patch(...)` is not taken for `unittest.mock.patch`.
- **Neither**: the name matches as written. A bare `sleep(1)` matches a `sleep` entry, and `mocker.patch(...)` matches `mocker.patch`.

A method call on some other object, such as `http_client.patch(...)`, does not match `patch`, because its full name is `http_client.patch`.

## List entries

In a rule's list of callees (see `## Configuration` in `--explain <rule>`), an entry takes one of these forms:

- `time.sleep`, `tokio::time::sleep`: a name, matched as described above.
- `*.assert_called_once`: the method on any receiver.
- `*().create_task`: the method on the result of any call.
- `asyncio.get_running_loop().create_task`: the method on the result of a call to that name.

## Known problems

- Only file-level imports and definitions count: the Python module body, including top-level `if`, `try` and `with` blocks, and root-level Rust `use` items and items. An import inside a function or a `mod` block is not seen, so the name matches as written.
- Parameters and assignments are not tracked. The pytest fixtures `mocker` and `monkeypatch` match only under those names, and `t = typing` does not make `t.cast` match `typing.cast`.
- Wildcard imports (`from x import *`, `use x::*`) bind nothing.
- String annotations such as `"list[int]"` are not parsed.
