use std::io;

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use super::Cli;

#[derive(clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    shell: Shell,
}

pub fn run(args: CompletionsArgs) {
    let mut cmd = Cli::command();
    generate(args.shell, &mut cmd, "bobbin", &mut io::stdout());
}
