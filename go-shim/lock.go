

package main

import (
	"fmt"
	"os"
	"path/filepath"
)

// indexLock holds an exclusive advisory lock on <layoutDir>/index.json.lock.
// It guards the read-modify-write cycle of index.json against concurrent
// processes on Linux, macOS, and Windows.
//
// The lock file is a permanent sidecar; it is never removed so every process
// can open it without a TOCTOU race.
type indexLock struct {
	f *os.File
}

// lockIndex opens (or creates) <layoutDir>/index.json.lock and acquires an
// exclusive lock, blocking until it is available.  The caller must call
// release() when the index update is complete.
func lockIndex(layoutDir string) (*indexLock, error) {
	path := filepath.Join(layoutDir, "index.json.lock")
	f, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o644)
	if err != nil {
		return nil, fmt.Errorf("open index lock file: %w", err)
	}
	if err := lockFile(f); err != nil {
		f.Close()
		return nil, fmt.Errorf("acquire index lock: %w", err)
	}
	return &indexLock{f: f}, nil
}

func (l *indexLock) release() {
	_ = unlockFile(l.f)
	l.f.Close()
}
