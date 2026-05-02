use std::path::PathBuf;

use clap::Args;

use crate::storage::OciStore;

#[derive(Args, Debug)]
pub struct TagArgs {
    /// Source reference (must exist locally)
    #[arg(value_name = "SOURCE")]
    pub source: String,

    /// New reference to create
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Local store directory (overrides default)
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,
}

pub fn run(args: &TagArgs) -> anyhow::Result<()> {
    let store_root = crate::default_store(args.store.as_deref())?;
    let store = OciStore::open(&store_root)?;

    let desc = store.find(&args.source)?;
    store.tag(desc, &args.target)?;
    println!("Tagged {} as {}", args.source, args.target);
    Ok(())
}
