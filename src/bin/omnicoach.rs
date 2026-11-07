//! Static analysis and command workflow coach binary.

omnicoach::architecture_component!(Bin);

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(omnicoach::cli::run_from_args(std::env::args_os()))
}
