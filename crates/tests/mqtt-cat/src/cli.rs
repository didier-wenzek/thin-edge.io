use clap::Parser;
use clap::Subcommand;

#[derive(Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Connect {
        #[arg(default_value = "127.0.0.1:1883")]
        host: String,
    },
    Bind {
        #[arg(default_value = "127.0.0.1:1883")]
        host: String,
    },
}

impl Args {
    pub fn host(&self) -> &str {
        match &self.command {
            Command::Connect { host } | Command::Bind { host } => host,
        }
    }
}
