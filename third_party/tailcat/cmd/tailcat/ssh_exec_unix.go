// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

//go:build unix && !ts_omit_ssh

package main

import (
	"os"
	"syscall"
)

// execSSH replaces the current process with the ssh client.
// It only returns on error.
func execSSH(sshExe string, argv []string) error {
	return syscall.Exec(sshExe, argv, os.Environ())
}
