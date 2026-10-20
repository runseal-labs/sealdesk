// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

//go:build (linux || darwin || windows) && !ts_omit_ssh

package tailcat

import "io"

// ServeRootedSFTPForTest serves one rooted SFTP session on rwc, so
// tests can drive the handlers with an SFTP client over a pipe
// without an SSH transport.
func ServeRootedSFTPForTest(rwc io.ReadWriteCloser, fsrv *FileService) error {
	srv, root, err := newRootedSFTPServer(rwc, fsrv)
	if err != nil {
		return err
	}
	defer root.Close()
	defer srv.Close()
	return srv.Serve()
}
