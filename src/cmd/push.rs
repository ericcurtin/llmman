use std::path::PathBuf;

use clap::Args;

use crate::ffi;

#[derive(Args, Debug)]
pub struct PushArgs {
    /// Registry reference (e.g. registry.example.com/mymodel:latest)
    #[arg(value_name = "REFERENCE")]
    pub reference: String,

    /// Local store directory (overrides default)
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,
}

pub fn run(args: &PushArgs) -> anyhow::Result<()> {
    let store_root = crate::default_store(args.store.as_deref())?;
    let reference = crate::shortnames::resolve(&args.reference);
    // Verify the image exists locally before attempting push
    let store = crate::storage::OciStore::open(&store_root)?;
    store.find(&reference)?;

    let layout_dir = store_root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("store path is not valid UTF-8"))?;

    ffi::push(layout_dir, &reference)?;
    println!("Pushed {}", reference);
    Ok(())
}
