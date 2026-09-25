//! VCS context abstractions and client implementations for Jujutsu (jj).

architecture_component!(CommandVcsAdapters);

/// Interface representing a client to query Jujutsu repository details.
pub trait JjClient {
    /// Queries the description of the given revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the VCS client fails to retrieve the description.
    fn get_commit_description(&self, revision: &str) -> Result<String, String>;
}

/// Command-line client executing `jj` commands as subprocesses.
pub struct JjCliClient;

impl JjClient for JjCliClient {
    /// Queries the description of the given revision using the `jj` CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if the `jj` command fails to run or returns a non-zero exit status.
    fn get_commit_description(&self, revision: &str) -> Result<String, String> {
        let output = std::process::Command::new("jj")
            .args(["log", "-r", revision, "-T", "description", "--no-graph"])
            .output()
            .map_err(|error| error.to_string())?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
