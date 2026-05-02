//go:build podman

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
	"os"
	"strings"
	"time"

	"github.com/vbauerster/mpb/v8"
	"github.com/vbauerster/mpb/v8/decor"
	commonauth "go.podman.io/common/pkg/auth"
	"go.podman.io/image/v5/copy"
	"go.podman.io/image/v5/signature"
	"go.podman.io/image/v5/transports/alltransports"
	"go.podman.io/image/v5/types"
)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func insecurePolicy() (*signature.PolicyContext, error) {
	policy := &signature.Policy{
		Default: signature.PolicyRequirements{
			signature.NewPRInsecureAcceptAnything(),
		},
	}
	return signature.NewPolicyContext(policy)
}

func tagFromRef(ref string) string {
	if i := strings.LastIndex(ref, ":"); i > strings.LastIndex(ref, "/") {
		return ref[i+1:]
	}
	return "latest"
}

// ---------------------------------------------------------------------------
// Exported CGO functions
// ---------------------------------------------------------------------------

// llmman_login stores credentials for a registry using the containers/common auth library.
//
//export llmman_login
func llmman_login(cServer, cUsername, cPassword *C.char) *C.char {
	server := C.GoString(cServer)
	username := C.GoString(cUsername)
	password := C.GoString(cPassword)

	sys := &types.SystemContext{}
	opts := &commonauth.LoginOptions{
		Username: username,
		Password: password,
	}
	if err := commonauth.Login(context.Background(), sys, opts, []string{server}); err != nil {
		return errResp(fmt.Errorf("login: %w", err))
	}
	return okResp("")
}

// llmman_logout removes credentials for a registry.
//
//export llmman_logout
func llmman_logout(cServer *C.char) *C.char {
	server := C.GoString(cServer)

	sys := &types.SystemContext{}
	opts := &commonauth.LogoutOptions{All: false}
	if err := commonauth.Logout(sys, opts, []string{server}); err != nil {
		return errResp(fmt.Errorf("logout: %w", err))
	}
	return okResp("")
}

// llmman_push pushes an image from a local OCI layout to a registry.
//
//export llmman_push
func llmman_push(cLayoutDir, cRef *C.char) *C.char {
	layoutDir := C.GoString(cLayoutDir)
	ref := C.GoString(cRef)
	tag := tagFromRef(ref)

	// Source: OCI layout directory
	srcStr := fmt.Sprintf("oci:%s:%s", layoutDir, tag)
	srcRef, err := alltransports.ParseImageName(srcStr)
	if err != nil {
		return errResp(fmt.Errorf("parse src ref %q: %w", srcStr, err))
	}

	// Destination: Docker registry
	dstStr := "docker://" + ref
	dstRef, err := alltransports.ParseImageName(dstStr)
	if err != nil {
		return errResp(fmt.Errorf("parse dst ref %q: %w", dstStr, err))
	}

	pctx, err := insecurePolicy()
	if err != nil {
		return errResp(fmt.Errorf("policy context: %w", err))
	}
	defer pctx.Destroy()

	_, err = copy.Image(context.Background(), pctx, dstRef, srcRef, &copy.Options{
		ReportWriter: os.Stderr,
	})
	if err != nil {
		return errResp(fmt.Errorf("copy image: %w", err))
	}
	return okResp("")
}

// llmman_pull pulls an image from a registry into a local OCI layout directory.
//
//export llmman_pull
func llmman_pull(cRef, cLayoutDir *C.char) *C.char {
	ref := C.GoString(cRef)
	layoutDir := C.GoString(cLayoutDir)

	// Normalize: append :latest if reference has no tag or digest
	if strings.LastIndex(ref, ":") <= strings.LastIndex(ref, "/") {
		ref = ref + ":latest"
	}

	tag := tagFromRef(ref)

	// Source: Docker registry
	srcStr := "docker://" + ref
	srcRef, err := alltransports.ParseImageName(srcStr)
	if err != nil {
		return errResp(fmt.Errorf("parse src ref %q: %w", srcStr, err))
	}

	// Ensure the OCI layout directory exists
	if err := os.MkdirAll(layoutDir, 0o755); err != nil {
		return errResp(fmt.Errorf("create layout dir: %w", err))
	}

	// Destination: OCI layout directory
	dstStr := fmt.Sprintf("oci:%s:%s", layoutDir, tag)
	dstRef, err := alltransports.ParseImageName(dstStr)
	if err != nil {
		return errResp(fmt.Errorf("parse dst ref %q: %w", dstStr, err))
	}

	pctx, err := insecurePolicy()
	if err != nil {
		return errResp(fmt.Errorf("policy context: %w", err))
	}
	defer pctx.Destroy()

	prog := mpb.New(mpb.WithOutput(os.Stderr))
	ch := make(chan types.ProgressProperties)
	bars := make(map[string]*mpb.Bar)
	progDone := make(chan struct{})
	go func() {
		defer close(progDone)
		for p := range ch {
			key := p.Artifact.Digest.String()
			switch p.Event {
			case types.ProgressEventNewArtifact:
				total := p.Artifact.Size
				if total < 0 {
					total = 0
				}
				short := p.Artifact.Digest.Hex()
				if len(short) > 12 {
					short = short[:12]
				}
				bar := prog.AddBar(total,
					mpb.BarFillerClearOnComplete(),
					mpb.PrependDecorators(
						decor.OnComplete(decor.Name("Pulling  "+short), "Pulled   "+short),
					),
					mpb.AppendDecorators(
						decor.OnComplete(decor.CountersKibiByte("% .1f / % .1f"), ""),
						decor.OnComplete(decor.Name("  "), ""),
						decor.OnComplete(decor.AverageSpeed(decor.SizeB1024(0), "% .1f"), ""),
					),
				)
				if total == 0 {
					bar.SetTotal(0, true)
				}
				bars[key] = bar
			case types.ProgressEventRead:
				if bar, ok := bars[key]; ok {
					bar.IncrInt64(int64(p.OffsetUpdate))
				}
			case types.ProgressEventDone:
				if bar, ok := bars[key]; ok {
					// SetTotal with triggerComplete forces current=total regardless of
					// timing, then fires done() — the OnComplete decorators take over.
					bar.SetTotal(int64(p.Offset), true)
					delete(bars, key)
				}
			case types.ProgressEventSkipped:
				if bar, ok := bars[key]; ok {
					bar.Abort(true)
					delete(bars, key)
					fmt.Fprintf(prog, "Cached   %s\n", p.Artifact.Digest.Hex()[:12])
				}
			}
		}
	}()

	_, err = copy.Image(context.Background(), pctx, dstRef, srcRef, &copy.Options{
		Progress:             ch,
		ProgressInterval:     200 * time.Millisecond,
		MaxParallelDownloads: 6,
	})
	close(ch)
	<-progDone
	prog.Wait()

	if err != nil {
		return errResp(fmt.Errorf("copy image: %w", err))
	}
	return okResp("")
}

// llmman_inspect fetches and returns the raw manifest JSON for a remote reference.
//
//export llmman_inspect
func llmman_inspect(cRef *C.char) *C.char {
	ref := C.GoString(cRef)

	srcStr := "docker://" + ref
	srcRef, err := alltransports.ParseImageName(srcStr)
	if err != nil {
		return errResp(fmt.Errorf("parse ref %q: %w", srcStr, err))
	}

	sys := &types.SystemContext{}
	img, err := srcRef.NewImage(context.Background(), sys)
	if err != nil {
		return errResp(fmt.Errorf("open image: %w", err))
	}
	defer img.Close()

	manifestData, _, err := img.Manifest(context.Background())
	if err != nil {
		return errResp(fmt.Errorf("fetch manifest: %w", err))
	}

	var buf bytes.Buffer
	if err := json.Indent(&buf, manifestData, "", "  "); err != nil {
		return okResp(string(manifestData))
	}
	return okResp(buf.String())
}

// Ensure io is used (imported via shared helpers but referenced here for the build)
var _ = io.Discard
