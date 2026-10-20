// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package tailcat

// ConnBlob is an alias for [Addr].
//
// Deprecated: use Addr instead.
type ConnBlob = Addr

// ConnBlob returns the tailcat address that clients use to connect to s.
//
// Deprecated: use [Server.TailcatAddr] instead.
func (s *Server) ConnBlob() ConnBlob {
	return s.TailcatAddr()
}

// ConnBlob serializes ci into a tailcat address.
//
// Deprecated: use [ConnInfo.Addr] instead.
func (ci *ConnInfo) ConnBlob() ConnBlob {
	return ci.Addr()
}

// ParseConnBlobRaw decodes an address into its wire form.
//
// Deprecated: use [ParseAddrRaw] instead.
func ParseConnBlobRaw(addr ConnBlob) (any, error) {
	return ParseAddrRaw(addr)
}

// ParseConnBlob decodes an address into a [ConnInfo].
//
// Deprecated: use [ParseAddr] instead.
func ParseConnBlob(addr ConnBlob) (ConnInfo, error) {
	return ParseAddr(addr)
}
