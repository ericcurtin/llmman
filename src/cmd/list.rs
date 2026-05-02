use std::time::SystemTime;

use clap::Args;

use crate::storage::OciStore;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Local store directory (overrides default)
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,
}

use std::path::PathBuf;

pub fn run(args: &ListArgs) -> anyhow::Result<()> {
    let store_root = crate::default_store(args.store.as_deref())?;
    let store = OciStore::open(&store_root)?;
    let images = store.list()?;

    if images.is_empty() {
        return Ok(());
    }

    let name_w = images.iter().map(|i| i.reference.len()).max().unwrap_or(4).max(4);

    println!(
        "{:<name_w$}    {:<16}    {:<10}    {}",
        "NAME", "ID", "SIZE", "MODIFIED",
        name_w = name_w,
    );

    for img in &images {
        println!(
            "{:<name_w$}    {:<16}    {:<10}    {}",
            img.reference,
            short_id(&img.digest),
            human_size(img.size),
            relative_time(img.modified_at),
            name_w = name_w,
        );
    }
    Ok(())
}

fn short_id(digest: &str) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    hex.chars().take(12).collect()
}

fn human_size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;
    const KB: u64 = 1_000;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} kB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn relative_time(t: Option<SystemTime>) -> String {
    let secs = match t {
        Some(t) => SystemTime::now()
            .duration_since(t)
            .unwrap_or_default()
            .as_secs(),
        None => return "unknown".into(),
    };
    match secs {
        s if s < 60          => "just now".into(),
        s if s < 3600        => format!("{} minutes ago", s / 60),
        s if s < 86400       => format!("{} hours ago", s / 3600),
        s if s < 86400 * 2   => "yesterday".into(),
        s if s < 86400 * 7   => format!("{} days ago", s / 86400),
        s if s < 86400 * 14  => "1 week ago".into(),
        s if s < 86400 * 30  => format!("{} weeks ago", s / (86400 * 7)),
        s if s < 86400 * 60  => "1 month ago".into(),
        s if s < 86400 * 365 => format!("{} months ago", s / (86400 * 30)),
        s if s < 86400 * 730 => "1 year ago".into(),
        s                    => format!("{} years ago", s / (86400 * 365)),
    }
}
