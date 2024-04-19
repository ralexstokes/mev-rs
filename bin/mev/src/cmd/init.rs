use clap::Args;

use crate::config::Config;

/// Include the `example.config.toml` file as a string constant
/// in the binary. The [`include_str`] macro will read the file
/// at compile time and include the contents in the binary.
const EXAMPLE_CONFIG: &str = std::include_str!("../../../../example.config.toml");

#[derive(Debug, Args)]
#[clap(about = "✨ (init) initializes a new config file")]
pub struct Command {
    /// The path to write the config file
    #[clap(env, default_value = "config.toml")]
    dest: String,
}

impl Command {
    pub fn execute(self) -> eyre::Result<()> {
        // Deserialize EXAMPLE_CONFIG into a Config struct
        let config: Config = toml::from_str(EXAMPLE_CONFIG)?;
        tracing::debug!("Loaded config template.");

        // Serialize the Config struct into a TOML string
        let contents = toml::to_string_pretty(&config)?;
        tracing::debug!("Serialized config to TOML.");

        // Write the TOML string to a file
        std::fs::write(&self.dest, contents)?;
        tracing::debug!("Wrote config to `{}`.", self.dest);

        Ok(())
    }
}
