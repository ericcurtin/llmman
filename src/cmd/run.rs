//! `llmman run` — interactive chat or one-shot prompt.
//!
//! Mirrors `ollama run`: interactive mode uses POST /api/chat with the full
//! message history; one-shot mode uses POST /api/generate.

use std::io::{self, IsTerminal, Write};

use anyhow::Context;
use clap::Args;

use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;
use tokio::time::{sleep, Duration, Instant};

const SERVER: &str = "http://127.0.0.1:17434";

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Model to run (short name or full reference)
    #[arg(value_name = "MODEL")]
    pub model: String,

    /// Prompt for one-shot mode; omit for interactive chat
    #[arg(value_name = "PROMPT", trailing_var_arg = true, allow_hyphen_values = true)]
    pub prompt: Vec<String>,
}

pub fn run(args: &RunArgs) -> anyhow::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(run_async(args))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

async fn run_async(args: &RunArgs) -> anyhow::Result<()> {
    let model = crate::shortnames::resolve(&args.model);
    let client = Client::new();

    ensure_server(&client, &model).await?;

    let prompt = args.prompt.join(" ");
    let interactive = prompt.is_empty() && io::stdin().is_terminal();

    if interactive {
        run_interactive(&client, &model).await
    } else {
        // One-shot: use the CLI prompt or read a single line from piped stdin.
        let p = if prompt.is_empty() {
            let mut line = String::new();
            let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
            reader.read_line(&mut line).await?;
            line.trim().to_string()
        } else {
            prompt
        };
        if !p.is_empty() {
            run_oneshot(&client, &model, &p).await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

async fn server_alive(client: &Client) -> bool {
    client
        .get(SERVER)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok()
}

/// Ensure llmman serve is running; start it in the background if not.
async fn ensure_server(client: &Client, model: &str) -> anyhow::Result<()> {
    if server_alive(client).await {
        return Ok(());
    }

    let exe = std::env::current_exe().context("could not resolve own executable")?;
    eprintln!("[llmman] starting serve...");
    tokio::process::Command::new(&exe)
        .arg("serve")
        .arg(model)
        .kill_on_drop(false) // keep running after llmman run exits
        .spawn()
        .context("spawn llmman serve")?;

    // Wait up to 60 s for the server to become ready.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if Instant::now() > deadline {
            anyhow::bail!("llmman serve did not start within 60 s");
        }
        if server_alive(client).await {
            return Ok(());
        }
        sleep(Duration::from_millis(300)).await;
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Msg {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: &'a [Msg],
    stream: bool,
}

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    message: Option<Msg>,
    #[serde(default)]
    done: bool,
}

#[derive(Serialize)]
struct GenReq<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct GenChunk {
    #[serde(default)]
    response: String,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    done: bool,
}

// ---------------------------------------------------------------------------
// Streaming NDJSON reader
// ---------------------------------------------------------------------------

/// Yield complete newline-terminated lines from an HTTP response as they
/// arrive, buffering incomplete lines across TCP chunks.
async fn stream_lines(
    resp: reqwest::Response,
    mut f: impl FnMut(&str),
) -> anyhow::Result<()> {
    use tokio::io::AsyncBufReadExt;
    use tokio_util::io::StreamReader;

    let body = resp
        .bytes_stream()
        .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
    let reader = StreamReader::new(body);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        f(&line);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// One-shot: POST /api/generate
// ---------------------------------------------------------------------------

async fn run_oneshot(client: &Client, model: &str, prompt: &str) -> anyhow::Result<()> {
    let url = format!("{SERVER}/api/generate");
    let body = GenReq { model, prompt, stream: true };

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("connect to llmman serve")?;

    if !resp.status().is_success() {
        let e = resp.text().await.unwrap_or_default();
        anyhow::bail!("{e}");
    }

    let mut thinking_open = false;
    let mut done = false;

    stream_lines(resp, |line| {
        let Ok(chunk) = serde_json::from_str::<GenChunk>(line) else { return };
        if let Some(ref t) = chunk.thinking {
            if !t.is_empty() {
                if !thinking_open {
                    eprint!("Thinking: ");
                    thinking_open = true;
                }
                eprint!("{t}");
            }
        }
        if !chunk.response.is_empty() && thinking_open {
            eprintln!();
            thinking_open = false;
        }
        if !chunk.response.is_empty() {
            print!("{}", chunk.response);
            io::stdout().flush().ok();
        }
        if chunk.done { done = true; }
    }).await?;

    println!("\n");
    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive: POST /api/chat with full message history
// ---------------------------------------------------------------------------

async fn run_interactive(client: &Client, model: &str) -> anyhow::Result<()> {
    let mut messages: Vec<Msg> = Vec::new();
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);

    eprintln!("Type a message, /bye to exit, or \"\"\" for multiline input.");

    loop {
        print!("> ");
        io::stdout().flush().ok();

        // Read one line (blocks until Enter or EOF).
        let mut raw = String::new();
        let n = reader.read_line(&mut raw).await?;
        if n == 0 {
            println!();
            break;
        }
        let first = strip_paste_markers(raw.trim_end_matches('\n').trim_end_matches('\r'));

        // ── Paste detection ────────────────────────────────────────────────────
        // Mirrors ollama readline's `Buffered() > 0` check:
        // when the user pastes, ALL lines land in the kernel TTY buffer at once.
        // The BufReader reads everything available in one syscall, so after
        // read_line() returns the first line, reader.buffer() already contains
        // the rest of the pasted content.  Keep draining it immediately.
        let content = if !reader.buffer().is_empty() {
            let mut buf = first;
            while !reader.buffer().is_empty() {
                let mut more = String::new();
                if reader.read_line(&mut more).await? == 0 { break; }
                let more = strip_paste_markers(more.trim_end_matches('\n').trim_end_matches('\r'));
                if !more.is_empty() {
                    buf.push('\n');
                    buf.push_str(&more);
                }
            }
            buf
        } else {
            first
        };

        // ── Slash commands ────────────────────────────────────────────────────
        match content.trim() {
            "" => continue,
            "/bye" | "/exit" => break,
            "/clear" => {
                messages.clear();
                eprintln!("Conversation cleared.");
                continue;
            }
            _ if content.trim().starts_with('/') => {
                eprintln!("Commands: /bye  /clear  \"\"\" (multiline)");
                continue;
            }
            _ => {}
        }

        // ── Triple-quote multiline (typed, not pasted) ────────────────────────
        let content = if content.trim_start().starts_with("\"\"\"") {
            let first_inner = content.trim_start().trim_start_matches("\"\"\"");
            if let Some(inner) = first_inner.strip_suffix("\"\"\"") {
                inner.to_string()
            } else {
                let mut buf = first_inner.to_string();
                buf.push('\n');
                loop {
                    print!(". ");
                    io::stdout().flush().ok();
                    let mut more = String::new();
                    let m = reader.read_line(&mut more).await?;
                    if m == 0 { break; }
                    let more = more.trim_end_matches('\n').trim_end_matches('\r');
                    if let Some(before) = more.strip_suffix("\"\"\"") {
                        buf.push_str(before);
                        break;
                    }
                    buf.push_str(more);
                    buf.push('\n');
                }
                buf.trim_end_matches('\n').to_string()
            }
        } else {
            content
        };

        if content.trim().is_empty() { continue; }

        messages.push(Msg { role: "user".into(), content, thinking: None });

        let assistant_content = chat_turn(client, model, &messages).await?;
        messages.push(Msg {
            role: "assistant".into(),
            content: assistant_content,
            thinking: None,
        });

        println!("\n");
    }

    Ok(())
}

/// Strip bracketed-paste escape markers that some terminals inject inline.
fn strip_paste_markers(s: &str) -> String {
    s.trim_start_matches("\x1b[200~")
     .trim_end_matches("\x1b[201~")
     .to_string()
}

/// Send one chat turn and stream the response to stdout.
/// Returns the full assembled assistant content.
async fn chat_turn(client: &Client, model: &str, messages: &[Msg]) -> anyhow::Result<String> {
    let url = format!("{SERVER}/api/chat");
    let body = ChatReq { model, messages, stream: true };

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("connect to llmman serve")?;

    if !resp.status().is_success() {
        let e = resp.text().await.unwrap_or_default();
        anyhow::bail!("{e}");
    }

    let mut content = String::new();
    let mut thinking_open = false;

    stream_lines(resp, |line| {
        let Ok(chunk) = serde_json::from_str::<ChatChunk>(line) else { return };
        if let Some(ref msg) = chunk.message {
            if let Some(ref t) = msg.thinking {
                if !t.is_empty() {
                    if !thinking_open {
                        eprint!("Thinking: ");
                        thinking_open = true;
                    }
                    eprint!("{t}");
                }
            }
            if !msg.content.is_empty() && thinking_open {
                eprintln!();
                thinking_open = false;
            }
            if !msg.content.is_empty() {
                print!("{}", msg.content);
                io::stdout().flush().ok();
                content.push_str(&msg.content);
            }
        }
    }).await?;

    Ok(content)
}
