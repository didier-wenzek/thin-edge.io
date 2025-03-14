use std::path::Path;

use crate::log::MaybeFancy;

/// A trait to be implemented by all tedge sub-commands.
///
/// A command encapsulates all the required parameters and provides an `execute()` method
/// to trigger the execution, plus a `description` of what is done by that command.
///
/// ```
/// use tedge::command::Command;
/// use tedge::log::MaybeFancy;
///
/// struct SayHello {
///     name: String,
/// };
///
/// impl Command for SayHello {
///     fn description(&self) -> String {
///        format!("say hello to '{}'", self.name)
///     }
///
///     fn execute(&self) -> Result<(), MaybeFancy<anyhow::Error>> {
///        println!("Hello {}!", self.name);
///        Ok(())
///     }
/// }
/// ```
///
/// If a command needs some context, say the tedge configuration,
/// this context can be provided to the command struct.
///
/// ```
/// use tedge::command::Command;
/// use tedge_config::TEdgeConfig;
/// use tedge_config::tedge_toml::ReadError;
/// use tedge_config::tedge_toml::ReadableKey;
/// use tedge::log::MaybeFancy;
///
/// struct GetConfigKey {
///     config: TEdgeConfig,
///     key: ReadableKey,
/// };
///
/// impl Command for GetConfigKey {
///     fn description(&self) -> String {
///        format!("get the value of the configuration key '{}'", self.key)
///     }
///
///     fn execute(&self) -> Result<(), MaybeFancy<anyhow::Error>> {
///        match self.config.read_string(&self.key) {
///             Ok(value) => println!("{}", value),
///             Err(ReadError::ConfigNotSet(_)) => eprintln!("The configuration key `{}` is not set", self.key),
///             Err(e) => return Err(MaybeFancy::Unfancy(e.into())),
///        };
///        Ok(())
///     }
/// }
/// ```
pub trait Command {
    /// Display that command to the user, telling what will be done.
    ///
    /// This description is displayed to the end user in case of an error, to give the context of that error.
    fn description(&self) -> String;

    /// Execute this command.
    ///
    /// The errors returned by this method must be concrete `anyhow::Error` values.
    /// The simplest way to implement a specific `anyhow::Error` type is to derive the `thiserror::Error`.
    /// Doing so, the command specific error type implements `Into<anyhow::Error>`
    /// and such errors can then be returned with no explicit conversion from the `run()` method.
    ///
    /// ```
    /// #[derive(thiserror::Error, Debug)]
    /// pub enum ConfigError {
    ///     #[error("Not a Thin Edge property: {key:?}")]
    ///     UnknownKey{key: String},
    /// }
    /// ```
    fn execute(&self) -> Result<(), MaybeFancy<anyhow::Error>>;

    /// Helper method to be used in the `BuildCommand` trait.
    ///
    /// The `BuildCommand::build_command()` method has to return a box around a new command.
    ///
    /// ```
    /// use tedge_config::tedge_toml::ReadableKey;
    /// use tedge_config::TEdgeConfig;
    /// use tedge::cli::config::GetConfigCommand;
    /// use tedge::ConfigError;
    /// use tedge::command::Command;
    ///
    /// struct SomeStruct;
    ///
    /// impl SomeStruct {
    ///     fn build_command(self, config: TEdgeConfig) -> Result<Box<dyn Command>, ConfigError> {
    ///         let cmd = GetConfigCommand { config, key: ReadableKey::MqttBindPort };
    ///         Ok(cmd.into_boxed())
    ///     }
    /// }
    /// ```
    fn into_boxed(self) -> Box<dyn Command>
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

/// A trait implemented by the tedge subcommands to build the actual command
/// using a combination of the parameters provided on the command line
/// and those from the configuration.
///
/// In practice, an implementation will also derives the `Structopt` trait.
///
/// ```
/// use tedge::command::*;
/// use tedge::cli::config::*;
/// use tedge::ConfigError;
/// use tedge_config::tedge_toml::tedge_config::*;
///
/// #[derive(clap::Parser, Debug)]
/// enum ConfigCmd {
///     /// Add new value (overwrite the value if the key exists).
///     Set { key: WritableKey, value: String },
///
///     /// Get value.
///     Get { key: ReadableKey },
/// }
///
/// impl BuildCommand for ConfigCmd {
///     fn build_command(self, context: BuildContext) -> Result<Box<dyn Command>, ConfigError> {
///         let cmd = match self {
///             ConfigCmd::Set { key, value } => SetConfigCommand {
///                 config_location: context.config_location,
///                 key,
///                 value,
///             }.into_boxed(),
///             ConfigCmd::Get { key } => GetConfigCommand {
///                 config: context.load_config()?,
///                 key,
///             }.into_boxed(),
///         };
///         Ok(cmd)
///     }
/// }
/// ```
pub trait BuildCommand {
    fn build_command(self, context: BuildContext) -> Result<Box<dyn Command>, crate::ConfigError>;
}

/// The context for `BuildCommand`
///
#[derive(Debug)]
pub struct BuildContext {
    pub config_location: tedge_config::TEdgeConfigLocation,
}

impl BuildContext {
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        let config_location = tedge_config::TEdgeConfigLocation::from_custom_root(config_dir);
        BuildContext { config_location }
    }

    pub fn load_config(&self) -> Result<tedge_config::TEdgeConfig, tedge_config::TEdgeConfigError> {
        tedge_config::TEdgeConfig::try_new(self.config_location.clone())
    }
}
