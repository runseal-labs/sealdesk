// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package tailcat

import _ "embed"

// README is the tailcat README.md, embedded so the CLI can print it
// with its readme subcommand. That lets people (and AI agents) with
// only the binary learn how to use it without web access.
//
//go:embed README.md
var README string
