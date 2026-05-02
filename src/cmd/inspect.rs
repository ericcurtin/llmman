use std::path::PathBuf;

use clap::Args;

use crate::ffi;
use crate::storage::OciStore;

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Image reference to inspect
    #[arg(value_name = "REFERENCE")]
    pub reference: String,

    /// Inspect a remote registry image instead of the local store
    #[arg(long)]
    pub remote: bool,

    /// Local store directory (overrides default)
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,
}

pub fn run(args: &InspectArgs) -> anyhow::Result<()> {
    if args.remote {
        let json = ffi::inspect_remote(&args.reference)?;
        println!("{}", json);
    } else {
        inspect_local(args)?;
    }
    Ok(())
}

fn inspect_local(args: &InspectArgs) -> anyhow::Result<()> {
    let store_root = crate::default_store(args.store.as_deref())?;
    let store = OciStore::open(&store_root)?;
    let desc = store.find(&args.reference)?;

    let manifest = store.read_manifest(&desc.digest)?;
    let out = serde_json::to_string_pretty(&manifest)?;
    println!("{}", out);
    Ok(())
}
