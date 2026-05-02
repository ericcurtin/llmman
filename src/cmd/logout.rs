use clap::Args;

use crate::ffi;

#[derive(Args, Debug)]
pub struct LogoutArgs {
    /// Registry server (e.g. registry.example.com)
    #[arg(value_name = "SERVER")]
    pub server: String,
}

pub fn run(args: &LogoutArgs) -> anyhow::Result<()> {
    ffi::logout(&args.server)?;
    println!("Logged out of {}", args.server);
    Ok(())
}
