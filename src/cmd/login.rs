use clap::Args;

use crate::ffi;

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Registry server (e.g. registry.example.com)
    #[arg(value_name = "SERVER")]
    pub server: String,

    /// Username
    #[arg(short, long)]
    pub username: String,

    /// Password (read from stdin if omitted)
    #[arg(short, long)]
    pub password: Option<String>,
}

pub fn run(args: &LoginArgs) -> anyhow::Result<()> {
    let password = match &args.password {
        Some(p) => p.clone(),
        None => {
            eprint!("Password: ");
            read_password_stdin()?
        }
    };
    ffi::login(&args.server, &args.username, &password)?;
    println!("Login succeeded for {}", args.server);
    Ok(())
}

/// Read a password from stdin, one line.
/// Does not suppress echo — callers wanting a TTY UX should pipe through a helper.
fn read_password_stdin() -> anyhow::Result<String> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}
