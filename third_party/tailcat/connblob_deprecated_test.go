// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package tailcat

import (
	"testing"

	"tailscale.com/types/key"
)

func TestDeprecatedConnBlobAPI(t *testing.T) {
	var _ func(*Server) ConnBlob = (*Server).ConnBlob

	ci := &ConnInfo{
		ServerPublic: NodePublic{key.NewNode().Public()},
		RegionID:     1,
	}
	var addr ConnBlob = ci.ConnBlob()

	if _, err := ParseConnBlob(addr); err != nil {
		t.Fatalf("ParseConnBlob: %v", err)
	}
	if _, err := ParseConnBlobRaw(addr); err != nil {
		t.Fatalf("ParseConnBlobRaw: %v", err)
	}
}
