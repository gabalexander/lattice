//! bell: rings sessions, a fixture for lattice's symbol index.

mod config;
mod session;

use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    /// The socket to talk to.
    #[arg(short = 'S', long)]
    socket: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ring a session.
    Ring {
        /// Wait for it to answer.
        #[arg(long)]
        wait: bool,
        name: String,
    },
    /// Stop a session.
    Stop { name: String },
    /// The sessions' settings.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print them.
    Show,
    /// Stop what's there.
    Stop {
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    let _ = Cli::parse();
    let _ = session::Session::new("bell");
}
