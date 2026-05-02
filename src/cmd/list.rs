use std::path::PathBuf;

use clap::Args;

use crate::storage::OciStore;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Local store directory (overrides default)
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,
}

pub fn run(args: &ListArgs) -> anyhow::Result<()> {
    let store_root = crate::default_store(args.store.as_deref())?;
    let store = OciStore::open(&store_root)?;
    let images = store.list()?;

    if images.is_empty() {
        println!("(no images)");
        return Ok(());
    }

    // Column widths
    let ref_w = images.iter().map(|i| i.reference.len()).max().unwrap_or(9).max(9);
    let dig_w = 19; // "sha256:<12 chars>…"

    println!(
        "{:<ref_w$}  {:<dig_w$}  SIZE",
        "REFERENCE",
        "DIGEST",
        ref_w = ref_w,
        dig_w = dig_w,
    );
    println!("{}", "-".repeat(ref_w + dig_w + 12));

    for img in &images {
        let short_digest = short_digest(&img.digest);
        let size_str = human_size(img.size);
        println!(
            "{:<ref_w$}  {:<dig_w$}  {}",
            img.reference,
            short_digest,
            size_str,
            ref_w = ref_w,
            dig_w = dig_w,
        );
    }
    Ok(())
}

fn short_digest(digest: &str) -> String {
    if let Some(hex) = digest.strip_prefix("sha256:") {
        format!("sha256:{}", &hex[..hex.len().min(12)])
    } else {
        digest.chars().take(19).collect()
    }
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}
