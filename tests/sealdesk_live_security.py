import glob
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time


def main():
    repository = Path(__file__).resolve().parents[1]
    binary = repository / "target/debug/sealdesk"
    candidates = [
        Path(path)
        for path in glob.glob(str(repository / "target/debug/deps/libsealdesk-*"))
        if Path(path).is_file() and os.access(path, os.X_OK)
    ]
    test_binary = max(candidates, key=lambda path: path.stat().st_mtime)
    ipc = socket.socket(socket.AF_UNIX)
    try:
        ipc.connect(f"/tmp/SealDesk-{os.geteuid()}/ipc")
    except OSError:
        pass
    else:
        raise RuntimeError("已有 SealDesk 服务正在运行；请在隔离账户中运行此测试")
    finally:
        ipc.close()

    with tempfile.TemporaryDirectory(prefix="sealdesk-security-") as directory:
        temporary = Path(directory)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(temporary / "host"))
        with (temporary / "server.log").open("w") as server_log:
            server = subprocess.Popen(
                [str(binary), "--server"],
                cwd=repository,
                env=environment,
                stdout=server_log,
                stderr=server_log,
                start_new_session=True,
            )
            try:
                code_file = temporary / "pairing.txt"
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if server.poll() is not None:
                        raise RuntimeError("隔离被控服务提前退出")
                    result = subprocess.run(
                        [str(binary), "--tc-pair"],
                        cwd=repository,
                        env=environment,
                        capture_output=True,
                        text=True,
                        timeout=5,
                    )
                    if result.returncode == 0 and "SEALDESK:" in result.stdout:
                        code_file.write_text(result.stdout.splitlines()[-1] + "\n")
                        code_file.chmod(0o600)
                        break
                    time.sleep(0.2)
                else:
                    raise RuntimeError("30 秒内未取得隔离服务的配对码")

                inodes = set()
                for descriptor in Path(f"/proc/{server.pid}/fd").iterdir():
                    try:
                        target = os.readlink(descriptor)
                    except FileNotFoundError:
                        continue
                    if target.startswith("socket:["):
                        inodes.add(target[8:-1])
                ports = []
                for row in Path("/proc/net/tcp").read_text().splitlines()[1:]:
                    fields = row.split()
                    if fields[3] == "0A" and fields[9] in inodes:
                        address, port = fields[1].split(":")
                        if address != "0100007F":
                            raise AssertionError("被控服务存在非 loopback TCP 监听")
                        ports.append(int(port, 16))
                assert len(ports) == 1, "未找到唯一的被控转发入口"
                with socket.create_connection(("127.0.0.1", ports[0]), timeout=2) as bypass:
                    bypass.sendall(bytes(32) + bytes([100, 64, 0, 1]))
                    assert bypass.recv(1) == b"", "本机进程绕过了 tailcat 认证"
                print("PASS local bypass refused before SealDesk login", flush=True)

                policies = subprocess.run(
                    [str(test_binary), "tailcat::live_tests::live_pairing_password_policies",
                     "--ignored", "--exact", "--nocapture", "--test-threads=1"],
                    cwd=repository,
                    env=dict(environment, SEALDESK_LIVE_CODE_FILE=str(code_file)),
                    capture_output=True,
                    text=True,
                    timeout=15,
                )
                if policies.returncode != 0 or "LIVE_PAIRING_POLICY_OK" not in policies.stdout:
                    locations = "\n".join(
                        line for line in policies.stderr.splitlines() if "panicked at" in line
                    )
                    raise AssertionError(
                        f"配对码密码策略检查失败（exit={policies.returncode}） {locations}"
                    )
                print("PASS click-only and permanent-only pairing codes omit passwords", flush=True)

                def login(identity, reject=False):
                    if not reject:
                        refreshed = subprocess.run(
                            [str(binary), "--tc-pair"],
                            cwd=repository,
                            env=environment,
                            capture_output=True,
                            text=True,
                            timeout=5,
                            check=True,
                        )
                        code_file.write_text(refreshed.stdout.splitlines()[-1] + "\n")
                    controller_environment = dict(
                        os.environ,
                        XDG_CONFIG_HOME=str(temporary / identity),
                        SEALDESK_LIVE_CODE_FILE=str(code_file),
                    )
                    controller_environment.pop("SEALDESK_LIVE_REJECT", None)
                    if reject:
                        controller_environment["SEALDESK_LIVE_REJECT"] = "1"
                    result = subprocess.run(
                        [
                            str(test_binary),
                            "tailcat::live_tests::live_tunnel_password_login",
                            "--ignored", "--exact", "--nocapture", "--test-threads=1",
                        ],
                        cwd=repository,
                        env=controller_environment,
                        capture_output=True,
                        text=True,
                        timeout=40,
                    )
                    lines = [line for line in result.stdout.splitlines() if "LIVE_LOGIN_" in line]
                    if result.returncode != 0 or not lines:
                        safe_error = "\n".join(
                            line for line in result.stderr.splitlines()
                            if "login rejected:" in line or "panicked at" in line
                        )
                        raise AssertionError(f"真实登录探针失败（exit={result.returncode}） {safe_error}")
                    print(lines[-1], flush=True)
                    return lines[-1]

                login("controller")
                forced_derp = os.environ.get("TS_DEBUG_ALWAYS_USE_DERP") == "true"
                if not forced_derp:
                    rejections = [login("attacker", reject=True) for _ in range(8)]
                    if not any("1 minute later" in line for line in rejections):
                        rejections.extend(login("attacker", reject=True) for _ in range(7))
                    assert any("1 minute later" in line for line in rejections), "攻击者未触发真实限流"
                login("independent-controller")
                if not forced_derp:
                    with ThreadPoolExecutor(max_workers=4) as workers:
                        rotating_rejections = list(workers.map(
                            lambda index: login(f"rotating-key-{index}", reject=True), range(24)
                        ))
                    limited = sum("Please try later" in line for line in rotating_rejections)
                    assert limited > 0, "更换控制端密钥没有触发总量限速"
                    print(f"PASS rotated-key password attempts share global budget: {limited}/24 throttled", flush=True)
                    time.sleep(5.1)
                    login("after-global-budget-recovery")
                    print("PASS global password budget recovers without restarting the server", flush=True)
                if forced_derp:
                    print("PASS forced DERP completes real password login within 10 seconds", flush=True)
                else:
                    print("PASS independent controller still logs in after repeated bad passwords", flush=True)
                original_address = code_file.read_text().split("A:", 1)[1].split(";", 1)[0]
                reset = subprocess.run(
                    [str(binary), "--tc-reset-address"], cwd=repository, env=environment,
                    capture_output=True, text=True, timeout=25,
                )
                assert reset.returncode == 0 and "地址已重置" in reset.stdout, "命令行重置地址失败"
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    refreshed = subprocess.run(
                        [str(binary), "--tc-pair"], cwd=repository, env=environment,
                        capture_output=True, text=True, timeout=5,
                    )
                    if refreshed.returncode == 0 and "SEALDESK:" in refreshed.stdout:
                        new_address = refreshed.stdout.split("A:", 1)[1].split(";", 1)[0]
                        if new_address != original_address:
                            break
                    time.sleep(0.2)
                else:
                    raise AssertionError("重置地址后未取得新的配对码")
                login("after-address-reset")
                print("PASS reset CLI revokes the old address and the new code completes secure password login", flush=True)
            finally:
                try:
                    os.killpg(server.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(server.pid, signal.SIGKILL)
                    server.wait(timeout=5)


if __name__ == "__main__":
    main()
