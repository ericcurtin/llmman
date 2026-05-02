use std::path::PathBuf;

use clap::Args;

use crate::ffi;

#[derive(Args, Debug)]
pub struct PullArgs {
    /// Registry reference to pull (e.g. registry.example.com/mymodel:latest)
    #[arg(value_name = "REFERENCE")]
    pub reference: String,

    /// Local store directory (overrides default)
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,
}

pub fn run(args: &PullArgs) -> anyhow::Result<()> {
    let store_root = crate::default_store(args.store.as_deref())?;
    let layout_dir = store_root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("store path is not valid UTF-8"))?;

    ffi::pull(&args.reference, layout_dir)?;
    println!("Pulled {}", args.reference);
    Ok(())
}
