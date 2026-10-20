// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

// Command printtags prints the build tag list for native builds of
// cmd/tailcat. It regenerates the checked-in build-tags.txt file and
// the -tags= line in .goreleaser.yaml, both of which a test in
// internal/buildtags keeps in sync.
package main

import (
	"fmt"

	"github.com/tailscale/tailcat/internal/buildtags"
)

func main() {
	fmt.Println(buildtags.ReleaseTags())
}
