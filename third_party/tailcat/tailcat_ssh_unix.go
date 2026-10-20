// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

//go:build (linux || darwin) && !ts_omit_ssh

package tailcat

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/user"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"

	"github.com/creack/pty"
	ssh "github.com/tailscale/gliderssh"
	"github.com/u-root/u-root/pkg/termios"
	gossh "golang.org/x/crypto/ssh"
	"golang.org/x/sys/unix"
)

// newSessionCommand returns an unstarted command running the user's login
// shell: interactively if rawCmd is empty, otherwise running rawCmd with the
// shell's -c flag. The returned command has Args, Dir, and the base
// environment set; the caller appends any client-provided environment.
func newSessionCommand(u *user.User, rawCmd string) *exec.Cmd {
	shell := loginShell(u)

	var args []string
	if rawCmd == "" {
		args = []string{shell, "-l"}
	} else {
		args = []string{shell, "-c", rawCmd}
	}

	cmd := exec.Command(args[0], args[1:]...)
	cmd.Dir = u.HomeDir
	cmd.Env = []string{
		"SHELL=" + shell,
		"USER=" + u.Username,
		"HOME=" + u.HomeDir,
		"PATH=" + defaultPath(u),
	}
	return cmd
}

// runWithPTY runs cmd attached to a pseudo-terminal.
func runWithPTY(sess ssh.Session, cmd *exec.Cmd, ptyReq ssh.Pty, winCh <-chan ssh.Window) {
	ptmx, tty, err := pty.Open()
	if err != nil {
		fmt.Fprintf(sess.Stderr(), "pty open: %v\r\n", err)
		sess.Exit(1)
		return
	}
	defer ptmx.Close()
	defer tty.Close()

	// Configure terminal modes from the SSH request.
	if rc, err := tty.SyscallConn(); err == nil {
		rc.Control(func(fd uintptr) {
			tios, err := termios.GTTY(int(fd))
			if err != nil {
				return
			}
			tios.Row = int(ptyReq.Window.Height)
			tios.Col = int(ptyReq.Window.Width)
			for c, v := range ptyReq.Modes {
				if c == gossh.TTY_OP_ISPEED {
					tios.Ispeed = int(v)
					continue
				}
				if c == gossh.TTY_OP_OSPEED {
					tios.Ospeed = int(v)
					continue
				}
				k, ok := opcodeShortName[c]
				if !ok {
					continue
				}
				if _, ok := tios.CC[k]; ok {
					tios.CC[k] = uint8(v)
					continue
				}
				if _, ok := tios.Opts[k]; ok {
					tios.Opts[k] = v > 0
					continue
				}
			}
			tios.STTY(int(fd))
		})
	}

	if ptyReq.Term != "" {
		cmd.Env = append(cmd.Env, "TERM="+ptyReq.Term)
	}

	cmd.SysProcAttr = &syscall.SysProcAttr{
		Setctty: true,
		Setsid:  true,
	}
	cmd.Stdin = tty
	cmd.Stdout = tty
	cmd.Stderr = tty

	if err := cmd.Start(); err != nil {
		fmt.Fprintf(sess.Stderr(), "start: %v\r\n", err)
		sess.Exit(1)
		return
	}
	tty.Close() // child owns the tty now

	// Handle window size changes. The goroutine runs until gliderssh
	// closes winCh, which happens only once the whole session channel
	// shuts down, after this function has returned and closed ptmx. It
	// therefore gets its own duplicated file descriptor rather than
	// racing the deferred ptmx.Close (and whatever reuses that fd).
	if winchFd, err := unix.Dup(int(ptmx.Fd())); err == nil {
		go func() {
			defer unix.Close(winchFd)
			for win := range winCh {
				unix.IoctlSetWinsize(winchFd, syscall.TIOCSWINSZ, &unix.Winsize{
					Row:    uint16(win.Height),
					Col:    uint16(win.Width),
					Xpixel: uint16(win.WidthPixels),
					Ypixel: uint16(win.HeightPixels),
				})
			}
		}()
	}

	// I/O: session ↔ pty master.
	go func() {
		io.Copy(ptmx, sess) // stdin
	}()
	io.Copy(sess, ptmx) // stdout (blocks until pty closes)

	if err := cmd.Wait(); err != nil {
		sess.Exit(exitCode(err))
		return
	}
	sess.Exit(0)
}

// loginShell returns the user's login shell.
func loginShell(u *user.User) string {
	switch runtime.GOOS {
	case "darwin":
		out, err := exec.Command("dscl", ".", "-read", filepath.Join("/Users", u.Username), "UserShell").Output()
		if err == nil {
			if s, ok := strings.CutPrefix(string(out), "UserShell: "); ok {
				return strings.TrimSpace(s)
			}
		}
	}
	if e := os.Getenv("SHELL"); e != "" {
		return e
	}
	return "/bin/sh"
}

// defaultPath returns the default PATH for the given user.
func defaultPath(u *user.User) string {
	if u.Uid == "0" {
		return "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
	}
	return "/usr/local/bin:/usr/bin:/bin"
}

// opcodeShortName maps SSH terminal mode opcodes to mnemonic names
// expected by the termios package.
var opcodeShortName = map[uint8]string{
	gossh.VINTR:         "intr",
	gossh.VQUIT:         "quit",
	gossh.VERASE:        "erase",
	gossh.VKILL:         "kill",
	gossh.VEOF:          "eof",
	gossh.VEOL:          "eol",
	gossh.VEOL2:         "eol2",
	gossh.VSTART:        "start",
	gossh.VSTOP:         "stop",
	gossh.VSUSP:         "susp",
	gossh.VDSUSP:        "dsusp",
	gossh.VREPRINT:      "rprnt",
	gossh.VWERASE:       "werase",
	gossh.VLNEXT:        "lnext",
	gossh.VFLUSH:        "flush",
	gossh.VSWTCH:        "swtch",
	gossh.VSTATUS:       "status",
	gossh.VDISCARD:      "discard",
	gossh.IGNPAR:        "ignpar",
	gossh.PARMRK:        "parmrk",
	gossh.INPCK:         "inpck",
	gossh.ISTRIP:        "istrip",
	gossh.INLCR:         "inlcr",
	gossh.IGNCR:         "igncr",
	gossh.ICRNL:         "icrnl",
	gossh.IUCLC:         "iuclc",
	gossh.IXON:          "ixon",
	gossh.IXANY:         "ixany",
	gossh.IXOFF:         "ixoff",
	gossh.IMAXBEL:       "imaxbel",
	gossh.IUTF8:         "iutf8",
	gossh.ISIG:          "isig",
	gossh.ICANON:        "icanon",
	gossh.XCASE:         "xcase",
	gossh.ECHO:          "echo",
	gossh.ECHOE:         "echoe",
	gossh.ECHOK:         "echok",
	gossh.ECHONL:        "echonl",
	gossh.NOFLSH:        "noflsh",
	gossh.TOSTOP:        "tostop",
	gossh.IEXTEN:        "iexten",
	gossh.ECHOCTL:       "echoctl",
	gossh.ECHOKE:        "echoke",
	gossh.PENDIN:        "pendin",
	gossh.OPOST:         "opost",
	gossh.OLCUC:         "olcuc",
	gossh.ONLCR:         "onlcr",
	gossh.OCRNL:         "ocrnl",
	gossh.ONOCR:         "onocr",
	gossh.ONLRET:        "onlret",
	gossh.CS7:           "cs7",
	gossh.CS8:           "cs8",
	gossh.PARENB:        "parenb",
	gossh.PARODD:        "parodd",
	gossh.TTY_OP_ISPEED: "tty_op_ispeed",
	gossh.TTY_OP_OSPEED: "tty_op_ospeed",
}
