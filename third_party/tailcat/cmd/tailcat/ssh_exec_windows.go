// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

//go:build windows && !ts_omit_ssh

package main

import (
	"errors"
	"os"
	"os/exec"
)

// execSSH runs the ssh client as a child process with inherited stdio,
// since Windows has no exec. On success it exits the whole process with
// ssh's exit status; it only returns on failure to run ssh at all.
func execSSH(sshExe string, argv []string) error {
	cmd := exec.Command(sshExe, argv[1:]...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) {
		os.Exit(exitErr.ExitCode())
	}
	if err != nil {
		return err
	}
	os.Exit(0)
	panic("unreachable")
}
