//go:build android

package main

import "C"

import (
	"encoding/json"
	"errors"
	"net"
	"sync"

	"tailscale.com/net/netmon"
)

type androidInterface struct {
	Name  string   `json:"name"`
	Index int      `json:"index"`
	MTU   int      `json:"mtu"`
	Flags uint     `json:"flags"`
	Addrs []string `json:"addrs"`
}

var androidInterfaces struct {
	sync.Mutex
	list []netmon.Interface
}

func init() {
	netmon.RegisterInterfaceGetter(func() ([]netmon.Interface, error) {
		androidInterfaces.Lock()
		defer androidInterfaces.Unlock()
		if androidInterfaces.list == nil {
			return nil, errors.New("android interface list not provided")
		}
		return androidInterfaces.list, nil
	})
}

// TcSetAndroidInterfaces receives JSON produced by the Android app from
// java.net.NetworkInterface, because Android denies netlink to app UIDs.
//
//export TcSetAndroidInterfaces
func TcSetAndroidInterfaces(payload *C.char) {
	var entries []androidInterface
	if err := json.Unmarshal([]byte(C.GoString(payload)), &entries); err != nil {
		return
	}
	list := make([]netmon.Interface, 0, len(entries))
	for _, entry := range entries {
		addrs := make([]net.Addr, 0, len(entry.Addrs))
		for _, text := range entry.Addrs {
			ip, network, err := net.ParseCIDR(text)
			if err != nil {
				continue
			}
			ones, _ := network.Mask.Size()
			addrs = append(addrs, &net.IPNet{IP: ip, Mask: net.CIDRMask(ones, len(ip)*8)})
		}
		list = append(list, netmon.Interface{
			Interface: &net.Interface{
				Name:  entry.Name,
				Index: entry.Index,
				MTU:   entry.MTU,
				Flags: net.Flags(entry.Flags),
			},
			AltAddrs: addrs,
		})
	}
	androidInterfaces.Lock()
	androidInterfaces.list = list
	androidInterfaces.Unlock()
}
