//go:build !podman

package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/containerd/containerd/v2/core/content"
	"github.com/containerd/containerd/v2/core/remotes"
	"github.com/containerd/containerd/v2/core/remotes/docker"
	dockerconfig "github.com/containerd/containerd/v2/core/remotes/docker/config"
	"github.com/containerd/errdefs"
	dockercliconfig "github.com/docker/cli/cli/config"
	clitypes "github.com/docker/cli/cli/config/types"
	digest "github.com/opencontainers/go-digest"
	specs "github.com/opencontainers/image-spec/specs-go"
	ocispec "github.com/opencontainers/image-spec/specs-go/v1"
)

// ---------------------------------------------------------------------------
// Credential helpers
// ---------------------------------------------------------------------------

func dockerCredentials(host string) (string, string, error) {
	cfg := dockercliconfig.LoadDefaultConfigFile(io.Discard)
	store := cfg.GetCredentialsStore(host)
	creds, err := store.Get(host)
	if err != nil {
		return "", "", nil // not an error — just not found
	}
	if creds.IdentityToken != "" {
		return "", creds.IdentityToken, nil
	}
	return creds.Username, creds.Password, nil
}

func newResolver(ctx context.Context) remotes.Resolver {
	return docker.NewResolver(docker.ResolverOptions{
		Hosts: dockerconfig.ConfigureHosts(ctx, dockerconfig.HostOptions{
			Credentials: dockerCredentials,
		}),
		Client: &http.Client{Timeout: 120 * time.Second},
	})
}

// ---------------------------------------------------------------------------
// OCI layout helpers
// ---------------------------------------------------------------------------

// blobPath returns the path for a blob in an OCI image layout directory.
func blobPath(layoutDir string, dgst digest.Digest) string {
	return filepath.Join(layoutDir, "blobs", dgst.Algorithm().String(), dgst.Hex())
}

// readBlob reads a blob from an OCI layout directory.
func readBlob(layoutDir string, dgst digest.Digest) ([]byte, error) {
	return os.ReadFile(blobPath(layoutDir, dgst))
}

// writeBlob atomically writes data to the OCI layout blobs directory,
// verifying the digest matches.  Returns the descriptor.
func writeBlob(layoutDir string, mediaType string, data []byte) (ocispec.Descriptor, error) {
	dgst := digest.FromBytes(data)
	dir := filepath.Join(layoutDir, "blobs", dgst.Algorithm().String())
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return ocispec.Descriptor{}, err
	}
	dest := filepath.Join(dir, dgst.Hex())
	// Skip if already present and correct size
	if fi, err := os.Stat(dest); err == nil && fi.Size() == int64(len(data)) {
		return ocispec.Descriptor{MediaType: mediaType, Digest: dgst, Size: int64(len(data))}, nil
	}
	tmp := dest + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return ocispec.Descriptor{}, err
	}
	if err := os.Rename(tmp, dest); err != nil {
		return ocispec.Descriptor{}, err
	}
	return ocispec.Descriptor{MediaType: mediaType, Digest: dgst, Size: int64(len(data))}, nil
}

// writeBlobStream writes a large stream to the OCI layout blobs directory,
// computing the digest on-the-fly.  Returns the descriptor with the computed digest.
func writeBlobStream(layoutDir string, mediaType string, r io.Reader, expectedSize int64) (ocispec.Descriptor, error) {
	tmp := filepath.Join(layoutDir, "blobs", "tmp-"+fmt.Sprintf("%d", time.Now().UnixNano()))
	if err := os.MkdirAll(filepath.Join(layoutDir, "blobs"), 0o755); err != nil {
		return ocispec.Descriptor{}, err
	}
	f, err := os.Create(tmp)
	if err != nil {
		return ocispec.Descriptor{}, err
	}
	digester := digest.Canonical.Digester()
	written, err := io.Copy(io.MultiWriter(f, digester.Hash()), r)
	f.Close()
	if err != nil {
		os.Remove(tmp)
		return ocispec.Descriptor{}, err
	}
	if expectedSize > 0 && written != expectedSize {
		os.Remove(tmp)
		return ocispec.Descriptor{}, fmt.Errorf("size mismatch: expected %d got %d", expectedSize, written)
	}
	dgst := digester.Digest()
	dir := filepath.Join(layoutDir, "blobs", dgst.Algorithm().String())
	if err := os.MkdirAll(dir, 0o755); err != nil {
		os.Remove(tmp)
		return ocispec.Descriptor{}, err
	}
	dest := filepath.Join(dir, dgst.Hex())
	if err := os.Rename(tmp, dest); err != nil {
		os.Remove(tmp)
		return ocispec.Descriptor{}, err
	}
	return ocispec.Descriptor{MediaType: mediaType, Digest: dgst, Size: written}, nil
}

// readIndex reads index.json from an OCI layout directory.
func readIndex(layoutDir string) (ocispec.Index, error) {
	data, err := os.ReadFile(filepath.Join(layoutDir, "index.json"))
	if err != nil {
		return ocispec.Index{}, err
	}
	var idx ocispec.Index
	return idx, json.Unmarshal(data, &idx)
}

// writeIndex writes index.json to an OCI layout directory.
func writeIndex(layoutDir string, idx ocispec.Index) error {
	data, err := json.MarshalIndent(idx, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(layoutDir, "index.json"), data, 0o644)
}

// ensureLayout initialises the OCI layout marker files if not present.
func ensureLayout(layoutDir string) error {
	if err := os.MkdirAll(layoutDir, 0o755); err != nil {
		return err
	}
	markerPath := filepath.Join(layoutDir, "oci-layout")
	if _, err := os.Stat(markerPath); os.IsNotExist(err) {
		marker := `{"imageLayoutVersion":"1.0.0"}`
		if err := os.WriteFile(markerPath, []byte(marker), 0o644); err != nil {
			return err
		}
	}
	indexPath := filepath.Join(layoutDir, "index.json")
	if _, err := os.Stat(indexPath); os.IsNotExist(err) {
		idx := ocispec.Index{
			Versioned: specs.Versioned{SchemaVersion: 2},
			MediaType: ocispec.MediaTypeImageIndex,
		}
		return writeIndex(layoutDir, idx)
	}
	return nil
}

// findManifestDesc looks up the manifest descriptor for a ref name in the index.
// Falls back to the first entry if there is only one and no explicit tag was given.
func findManifestDesc(idx ocispec.Index, refName string) (ocispec.Descriptor, error) {
	for _, m := range idx.Manifests {
		if m.Annotations != nil {
			if m.Annotations[ocispec.AnnotationRefName] == refName {
				return m, nil
			}
		}
	}
	// Fallback: single-entry index
	if len(idx.Manifests) == 1 {
		return idx.Manifests[0], nil
	}
	return ocispec.Descriptor{}, fmt.Errorf("no manifest found for %q", refName)
}

// ---------------------------------------------------------------------------
// ociProvider implements content.Provider backed by an OCI layout directory.
// ---------------------------------------------------------------------------

type ociProvider struct{ dir string }

func (p *ociProvider) ReaderAt(ctx context.Context, desc ocispec.Descriptor) (content.ReaderAt, error) {
	path := blobPath(p.dir, desc.Digest)
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("blob %s: %w", desc.Digest, err)
	}
	fi, err := f.Stat()
	if err != nil {
		f.Close()
		return nil, err
	}
	return &fileReaderAt{f: f, size: fi.Size()}, nil
}

type fileReaderAt struct {
	f    *os.File
	size int64
}

func (r *fileReaderAt) ReadAt(p []byte, off int64) (int, error) { return r.f.ReadAt(p, off) }
func (r *fileReaderAt) Close() error                            { return r.f.Close() }
func (r *fileReaderAt) Size() int64                             { return r.size }

// pushBlob pushes a single blob from the OCI layout to the registry pusher.
func pushBlob(ctx context.Context, pusher remotes.Pusher, provider *ociProvider, desc ocispec.Descriptor) error {
	cw, err := pusher.Push(ctx, desc)
	if err != nil {
		if errdefs.IsAlreadyExists(err) {
			return nil
		}
		return err
	}
	defer cw.Close()

	ra, err := provider.ReaderAt(ctx, desc)
	if err != nil {
		return err
	}
	defer ra.Close()

	return content.Copy(ctx, cw, io.NewSectionReader(ra, 0, ra.Size()), desc.Size, desc.Digest)
}

// tagFromRef extracts the tag or short name portion of a registry reference.
// e.g. "registry.example.com/repo:tag" → "tag"
//
//	"registry.example.com/repo"       → "latest"
func tagFromRef(ref string) string {
	if i := strings.LastIndex(ref, ":"); i > strings.LastIndex(ref, "/") {
		return ref[i+1:]
	}
	return "latest"
}

// ---------------------------------------------------------------------------
// Exported CGO functions
// ---------------------------------------------------------------------------

// llmman_login stores credentials for a registry in the Docker credential store.
//
//export llmman_login
func llmman_login(cServer, cUsername, cPassword *C.char) *C.char {
	server := C.GoString(cServer)
	username := C.GoString(cUsername)
	password := C.GoString(cPassword)

	cfg := dockercliconfig.LoadDefaultConfigFile(io.Discard)
	store := cfg.GetCredentialsStore(server)

	if err := store.Store(clitypes.AuthConfig{
		ServerAddress: server,
		Username:      username,
		Password:      password,
	}); err != nil {
		return errResp(fmt.Errorf("store credentials: %w", err))
	}
	if err := cfg.Save(); err != nil {
		return errResp(fmt.Errorf("save config: %w", err))
	}
	return okResp("")
}

// llmman_logout removes credentials for a registry from the Docker credential store.
//
//export llmman_logout
func llmman_logout(cServer *C.char) *C.char {
	server := C.GoString(cServer)

	cfg := dockercliconfig.LoadDefaultConfigFile(io.Discard)
	store := cfg.GetCredentialsStore(server)
	if err := store.Erase(server); err != nil {
		return errResp(fmt.Errorf("erase credentials: %w", err))
	}
	if err := cfg.Save(); err != nil {
		return errResp(fmt.Errorf("save config: %w", err))
	}
	return okResp("")
}

// llmman_push pushes an image from a local OCI layout directory to a registry.
// layoutDir is the path to the OCI layout root; ref is the full registry reference.
//
//export llmman_push
func llmman_push(cLayoutDir, cRef *C.char) *C.char {
	layoutDir := C.GoString(cLayoutDir)
	ref := C.GoString(cRef)
	ctx := context.Background()

	// Locate the manifest in the local index
	idx, err := readIndex(layoutDir)
	if err != nil {
		return errResp(fmt.Errorf("read OCI index: %w", err))
	}
	tag := tagFromRef(ref)
	manifestDesc, err := findManifestDesc(idx, tag)
	if err != nil {
		return errResp(err)
	}

	// Read manifest
	manifestData, err := readBlob(layoutDir, manifestDesc.Digest)
	if err != nil {
		return errResp(fmt.Errorf("read manifest blob: %w", err))
	}
	var manifest ocispec.Manifest
	if err := json.Unmarshal(manifestData, &manifest); err != nil {
		return errResp(fmt.Errorf("parse manifest: %w", err))
	}

	resolver := newResolver(ctx)
	pusher, err := resolver.Pusher(ctx, ref)
	if err != nil {
		return errResp(fmt.Errorf("create pusher: %w", err))
	}
	provider := &ociProvider{dir: layoutDir}

	// Push layers
	for _, layer := range manifest.Layers {
		if err := pushBlob(ctx, pusher, provider, layer); err != nil {
			return errResp(fmt.Errorf("push layer %s: %w", layer.Digest, err))
		}
	}
	// Push config
	if err := pushBlob(ctx, pusher, provider, manifest.Config); err != nil {
		return errResp(fmt.Errorf("push config: %w", err))
	}
	// Push manifest
	if err := pushBlob(ctx, pusher, provider, manifestDesc); err != nil {
		return errResp(fmt.Errorf("push manifest: %w", err))
	}
	return okResp("")
}

// llmman_pull pulls an image from a registry into a local OCI layout directory.
//
//export llmman_pull
func llmman_pull(cRef, cLayoutDir *C.char) *C.char {
	ref := C.GoString(cRef)
	layoutDir := C.GoString(cLayoutDir)
	ctx := context.Background()

	if err := ensureLayout(layoutDir); err != nil {
		return errResp(fmt.Errorf("init OCI layout: %w", err))
	}

	resolver := newResolver(ctx)
	name, manifestDesc, err := resolver.Resolve(ctx, ref)
	if err != nil {
		return errResp(fmt.Errorf("resolve %s: %w", ref, err))
	}
	fetcher, err := resolver.Fetcher(ctx, name)
	if err != nil {
		return errResp(fmt.Errorf("create fetcher: %w", err))
	}

	// Fetch and store manifest
	rc, err := fetcher.Fetch(ctx, manifestDesc)
	if err != nil {
		return errResp(fmt.Errorf("fetch manifest: %w", err))
	}
	manifestData, err := io.ReadAll(rc)
	rc.Close()
	if err != nil {
		return errResp(fmt.Errorf("read manifest: %w", err))
	}
	if _, err := writeBlob(layoutDir, manifestDesc.MediaType, manifestData); err != nil {
		return errResp(fmt.Errorf("write manifest blob: %w", err))
	}

	// Decode manifest to learn about layers and config
	var manifest ocispec.Manifest
	if err := json.Unmarshal(manifestData, &manifest); err != nil {
		// Could be an image index — store and return
		if err2 := updateIndex(layoutDir, ref, manifestDesc); err2 != nil {
			return errResp(err2)
		}
		return okResp("")
	}

	// Fetch config
	configRC, err := fetcher.Fetch(ctx, manifest.Config)
	if err != nil {
		return errResp(fmt.Errorf("fetch config: %w", err))
	}
	configData, readErr := io.ReadAll(configRC)
	configRC.Close()
	if readErr != nil {
		return errResp(fmt.Errorf("read config: %w", readErr))
	}
	if _, err := writeBlob(layoutDir, manifest.Config.MediaType, configData); err != nil {
		return errResp(fmt.Errorf("write config blob: %w", err))
	}

	// Fetch layers (streamed to avoid large allocations)
	for _, layer := range manifest.Layers {
		layerRC, err := fetcher.Fetch(ctx, layer)
		if err != nil {
			return errResp(fmt.Errorf("fetch layer %s: %w", layer.Digest, err))
		}
		_, writeErr := writeBlobStream(layoutDir, layer.MediaType, layerRC, layer.Size)
		layerRC.Close()
		if writeErr != nil {
			return errResp(fmt.Errorf("write layer %s: %w", layer.Digest, writeErr))
		}
	}

	if err := updateIndex(layoutDir, ref, manifestDesc); err != nil {
		return errResp(err)
	}
	return okResp("")
}

// updateIndex adds or replaces the manifest entry in index.json.
func updateIndex(layoutDir, ref string, manifestDesc ocispec.Descriptor) error {
	idx, err := readIndex(layoutDir)
	if err != nil {
		// New index
		idx = ocispec.Index{
			Versioned: specs.Versioned{SchemaVersion: 2},
			MediaType: ocispec.MediaTypeImageIndex,
		}
	}
	tag := tagFromRef(ref)
	if manifestDesc.Annotations == nil {
		manifestDesc.Annotations = map[string]string{}
	}
	manifestDesc.Annotations[ocispec.AnnotationRefName] = tag

	// Replace existing entry with same ref name, or append
	replaced := false
	for i, m := range idx.Manifests {
		if m.Annotations != nil && m.Annotations[ocispec.AnnotationRefName] == tag {
			idx.Manifests[i] = manifestDesc
			replaced = true
			break
		}
	}
	if !replaced {
		idx.Manifests = append(idx.Manifests, manifestDesc)
	}
	return writeIndex(layoutDir, idx)
}

// llmman_inspect fetches and returns the raw manifest JSON for a remote reference.
//
//export llmman_inspect
func llmman_inspect(cRef *C.char) *C.char {
	ref := C.GoString(cRef)
	ctx := context.Background()

	resolver := newResolver(ctx)
	name, manifestDesc, err := resolver.Resolve(ctx, ref)
	if err != nil {
		return errResp(fmt.Errorf("resolve %s: %w", ref, err))
	}
	fetcher, err := resolver.Fetcher(ctx, name)
	if err != nil {
		return errResp(fmt.Errorf("create fetcher: %w", err))
	}
	rc, err := fetcher.Fetch(ctx, manifestDesc)
	if err != nil {
		return errResp(fmt.Errorf("fetch manifest: %w", err))
	}
	data, err := io.ReadAll(rc)
	rc.Close()
	if err != nil {
		return errResp(fmt.Errorf("read manifest: %w", err))
	}

	// Pretty-print
	var buf bytes.Buffer
	if err := json.Indent(&buf, data, "", "  "); err != nil {
		return okResp(string(data))
	}
	return okResp(buf.String())
}
