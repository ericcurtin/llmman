//go:build windows && !podman

package main

import (
	"os"

	"golang.org/x/sys/windows"
)

func lockFile(f *os.File) error {
	var ol windows.Overlapped
	// Lock the entire file (nNumberOfBytesToLock{Low,High} = maxuint32).
	return windows.LockFileEx(
		windows.Handle(f.Fd()),
		windows.LOCKFILE_EXCLUSIVE_LOCK,
		0, ^uint32(0), ^uint32(0),
		&ol,
	)
}

func unlockFile(f *os.File) error {
	var ol windows.Overlapped
	return windows.UnlockFileEx(
		windows.Handle(f.Fd()),
		0, ^uint32(0), ^uint32(0),
		&ol,
	)
}
