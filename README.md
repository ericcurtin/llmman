# llmman

A command-line tool for managing LLM model images in OCI registries.
Models are packaged as standard OCI artifacts and stored in any compatible registry (Docker Hub, GHCR, ECR, self-hosted, etc.).

`llmman` is written in Rust and uses the same registry transport libraries as Docker or Podman — selected at compile time — without spawning either as a subprocess.

## Commands

| Command | Description |
|---------|-------------|
| `build` | Package model files into a local OCI image |
| `login` | Log in to a container registry |
| `logout` | Log out from a container registry |
| `push` | Push a local image to a registry |
| `pull` | Pull an image from a registry to the local store |
| `list` | List locally stored images |
| `rm` | Remove a local image |
| `inspect` | Show the manifest of a local or remote image |
| `tag` | Create a new local tag pointing to an existing image |

## Installation

### Pre-built binaries

Download the latest binary for your platform from the [releases page](../../releases).

| Platform | File |
|----------|------|
| Linux x86_64 | `llmman-x86_64-unknown-linux-gnu` |
| Linux aarch64 | `llmman-aarch64-unknown-linux-gnu` |
| macOS aarch64 | `llmman-aarch64-apple-darwin` |
| Windows x86_64 | `llmman-x86_64-pc-windows-msvc.exe` |
| Windows aarch64 | `llmman-aarch64-pc-windows-msvc.exe` |

### Build from source

**Prerequisites**

- Rust 1.75+ (`rustup` recommended)
- Go 1.22+
- A C compiler (gcc on Linux, Xcode CLT on macOS, MSVC on Windows)

```
cargo build --release
```

The build script (`build.rs`) compiles the Go transport shim and links it into the binary automatically.

## Usage

### Build a model image

Package every file in a directory as a set of OCI layers:

```
llmman build -t registry.example.com/mymodel:v1 ./path/to/model/
```

Add metadata labels:

```
llmman build -t registry.example.com/mymodel:v1 \
  -l org.llmman.format=gguf \
  -l org.llmman.quantization=q4_k_m \
  ./path/to/model/
```

### Authenticate

```
llmman login registry.example.com -u alice
# prompts for password

llmman login registry.example.com -u alice -p mypassword

llmman logout registry.example.com
```

Credentials are stored in the Docker credential store (`~/.docker/config.json`), so they are shared with `docker` and `podman` when using the default Docker backend.

### Push and pull

```
llmman push registry.example.com/mymodel:v1

llmman pull registry.example.com/mymodel:v1
```

### List local images

```
llmman list
```

```
REFERENCE                        DIGEST               SIZE
------------------------------------------------------------------
registry.example.com/mymodel:v1  sha256:a98f17b4903f  4.2 GiB
```

### Tag

Create an additional local reference without re-building:

```
llmman tag registry.example.com/mymodel:v1 registry.example.com/mymodel:latest
```

### Remove

```
llmman rm registry.example.com/mymodel:v1

# remove multiple at once
llmman rm registry.example.com/mymodel:v1 registry.example.com/mymodel:latest
```

### Inspect

Local manifest:

```
llmman inspect registry.example.com/mymodel:v1
```

Remote manifest (fetches directly from the registry, no local copy needed):

```
llmman inspect --remote registry.example.com/mymodel:v1
```

### Custom store location

All commands accept `--store <DIR>` to override the default local store location.

Default locations:

| OS | Path |
|----|------|
| Linux | `~/.local/share/llmman/store` |
| macOS | `~/Library/Application Support/llmman/store` |
| Windows | `%LOCALAPPDATA%\llmman\store` |

The store uses the [OCI Image Layout](https://github.com/opencontainers/image-spec/blob/main/image-layout.md) format, so it is readable by `docker`, `podman`, and `skopeo` directly.

## Transport backends

The registry transport is provided by a compiled-in Go shim. Two backends are available, selected at build time via a Cargo feature flag.

### Docker (default)

Uses [`github.com/containerd/containerd/v2/core/remotes/docker`](https://github.com/containerd/containerd) — the same OCI registry resolver used by moby/Docker — together with `github.com/docker/cli/cli/config` for credential storage.

```
cargo build --release                          # docker backend (default)
cargo build --release --features docker        # explicit
```

### Podman

Uses [`go.podman.io/image/v5`](https://github.com/containers/image) and [`go.podman.io/common/pkg/auth`](https://github.com/containers/common) — the same libraries Podman uses internally.

```
cargo build --release --no-default-features --features podman
```

> The Podman backend is not built for Windows targets in CI, as `containers/image` has limited Windows support.

## Local image format

Each file in the source directory becomes a separate uncompressed tar layer (`application/vnd.oci.image.layer.v1.tar`) with its relative path recorded in the `org.opencontainers.image.title` annotation. A JSON config blob records the creation timestamp, host architecture, OS, and any user-supplied labels.

This structure is intentionally simple and compatible with any OCI-compliant tool.

## CI

GitHub Actions builds natively on all five supported targets on every push and pull request:

| Target | Runner |
|--------|--------|
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-15` |
| `x86_64-pc-windows-msvc` | `windows-2025` |
| `aarch64-pc-windows-msvc` | `windows-11-arm` |

Tag pushes matching `v*` additionally publish a GitHub Release with all five binaries attached.

## License

Apache-2.0 — see [LICENSE](LICENSE).
