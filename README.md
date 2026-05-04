# llmman

A command-line tool for managing and serving LLM models using OCI registries.
Models are packaged as standard OCI artifacts and stored in any compatible registry (Docker Hub, GHCR, quay, self-hosted, etc.).
`llmman serve` exposes Ollama-, OpenAI-, and Anthropic-compatible HTTP APIs backed by `llama-server` subprocesses.

`llmman` is written in Rust and uses the same registry transport libraries as Docker or Podman selected at compile time without spawning either as a subprocess.

## Commands

| Command | Description |
|---------|-------------|
| `serve`   | Start an inference server (Ollama / OpenAI / Anthropic APIs) |
| `pull`    | Pull a model from a registry or HuggingFace |
| `list`    | List locally stored models |
| `build`   | Package model files into a local OCI image |
| `push`    | Push a local image to a registry |
| `rm`      | Remove a local image |
| `tag`     | Create a new local tag pointing to an existing image |
| `inspect` | Show the manifest of a local or remote image |
| `login`   | Log in to a container registry |
| `logout`  | Log out from a container registry |

## Quick start

### Pull a model

Pull directly from HuggingFace using short names:

```
llmman pull qwen3.5:0.8b-q4_K_M
```

Or use the full reference:

```
llmman pull hf.co/unsloth/Qwen3.5-0.8B-GGUF:Q4_K_M
```

### List local models

```
llmman ls
```

```
NAME                                      ID              SIZE      MODIFIED
hf.co/unsloth/Qwen3.5-0.8B-GGUF:latest    b55c07040368    532.5 MB  2 hours ago
```

### Serve

Start the inference server. Requires `llama-server` from [llama.cpp](https://github.com/ggml-org/llama.cpp) to be on `PATH`.

```
llmman serve
```

Optionally pass a model to pre-load on startup:

```
llmman serve hf.co/unsloth/Qwen3.5-0.8B-GGUF:latest
```

The server listens on `127.0.0.1:17434` and exposes:

| API | Endpoints |
|-----|-----------|
| Ollama | `/api/generate`, `/api/chat`, `/api/tags`, `/api/show`, `/api/pull`, `/api/ps`, `/api/delete` |
| OpenAI | `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings`, `/v1/models` |
| Anthropic | `/v1/messages` |

Use it as an Ollama-compatible server:

```
OLLAMA_HOST=127.0.0.1:17434 ollama run hf.co/unsloth/Qwen3.5-0.8B-GGUF
```

Or with any OpenAI-compatible client:

```python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:17434/v1", api_key="unused")
response = client.chat.completions.create(
    model="hf.co/unsloth/Qwen3.5-0.8B-GGUF:latest",
    messages=[{"role": "user", "content": "Hello"}],
)
```

Models are loaded on demand. Each model gets its own `llama-server` subprocess on a random loopback port; subsequent requests reuse the running process.

## Short names

`shortnames.conf` maps friendly names to full registry references:

```
llmman pull qwen3.5:0.8b          # → hf.co/unsloth/Qwen3.5-0.8B-GGUF
llmman pull gemma4:e4b-it-q4_K_M  # → hf.co/unsloth/gemma-4-E4B-it-GGUF:Q4_K_M
llmman pull granite4.1:8b-q4_K_M  # → hf.co/unsloth/granite-4.1-8b-GGUF:Q4_K_M
```

Short names work with all commands: `pull`, `push`, `rm`, `tag`, `inspect`, and `serve`.

## Registry operations

### Authenticate

```
llmman login registry.example.com -u alice
# prompts for password

llmman logout registry.example.com
```

Credentials are stored in the Docker credential store (`~/.docker/config.json`), shared with `docker` and `podman`.

### Push and pull (OCI registries)

```
llmman push registry.example.com/mymodel:v1

llmman pull registry.example.com/mymodel:v1
```

### Build a model image

Package every file in a directory as a set of OCI layers:

```
llmman build -t registry.example.com/mymodel:v1 ./path/to/model/
```

### Tag, remove, inspect

```
llmman tag registry.example.com/mymodel:v1 registry.example.com/mymodel:latest

llmman rm registry.example.com/mymodel:v1

llmman inspect registry.example.com/mymodel:v1
llmman inspect --remote registry.example.com/mymodel:v1
```

## Store location

Default locations (override with `--store <DIR>`):

| OS | Path |
|----|------|
| Linux, macOS | `~/.local/share/llmman/store` |
| Windows | `%LOCALAPPDATA%\llmman\store` |

The store uses [OCI Image Layout](https://github.com/opencontainers/image-spec/blob/main/image-layout.md), readable by `docker`, `podman`, and `skopeo` directly.

## Transport backends

The registry transport is a compiled-in Go shim. Two backends are available via Cargo feature flags.

### Docker (default)

Uses [`github.com/containerd/containerd`](https://github.com/containerd/containerd) — the same OCI resolver used by Docker.

```
cargo build --release
```

### Podman

Uses [`go.podman.io/image/v5`](https://github.com/containers/image) — the same library Podman uses internally.

```
cargo build --release --no-default-features --features podman
```

## Installation

### Build from source

Requires Rust and Go toolchains.

```
git clone https://github.com/ericcurtin/llmman
cd llmman
cargo build --release
```
