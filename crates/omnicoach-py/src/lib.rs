//! `CPython` `abi3-py310` extension entrypoint delegating CLI execution to [`omnicoach::cli`].

use std::ffi::OsString;

use pyo3::prelude::*;

/// Registers the internal `_omnicoach` native extension module.
#[pymodule]
fn _omnicoach(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_cli, module)?)?;
    Ok(())
}

/// Executes the `omnicoach` CLI from `args`, releasing the Python GIL during execution
/// and propagating pending Python signals (such as `SIGINT` / `KeyboardInterrupt`) on return.
#[pyfunction]
fn run_cli(py: Python<'_>, args: Vec<OsString>) -> PyResult<u8> {
    let exit_code = py.detach(|| omnicoach::cli::run_from_args(args));
    py.check_signals()?;
    Ok(exit_code)
}
