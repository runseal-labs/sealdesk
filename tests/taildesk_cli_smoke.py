import os
from pathlib import Path
import subprocess
import tempfile


def main():
    repository = Path(__file__).resolve().parents[1]
    binary = repository / "target/debug/rustdesk"
    secret = "cli-secret-must-not-be-logged"
    address = "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-"
    expired = f"TAILDESK:V:1;A:{address};P:{secret};E:1;;"
    candidates = [
        path for path in (repository / "target/debug/deps").glob("librustdesk-*")
        if path.is_file() and os.access(path, os.X_OK)
    ]
    test_binary = max(candidates, key=lambda path: path.stat().st_mtime)
    with tempfile.TemporaryDirectory(prefix="taildesk-cli-") as directory:
        temporary = Path(directory)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(temporary))
        image = temporary / "配对 图.png"
        image.write_bytes(b"not a QR image")
        cases = [
            (["--tc-reset-address", "unexpected"], "用法"),
            (["--host", "--control"], "用法"),
            (["--host", "--no-server"], "用法"),
            (["--control", "--host"], "需要配对码"),
            (["--control", expired], "expired"),
            (["--control", str(temporary / "missing.png")], "无法读取"),
            (["--control", str(temporary)], "普通文件"),
            (["--control", str(image)], "需要 Flutter 版本"),
        ]
        for arguments, expected in cases:
            result = subprocess.run(
                [str(binary), *arguments], cwd=repository, env=environment,
                capture_output=True, text=True, timeout=10,
            )
            assert result.returncode == 1, "非法命令行未以失败状态退出"
            assert expected in result.stderr, "命令行缺少预期错误原因"
            assert secret not in result.stdout + result.stderr, "凭证泄漏到命令行输出"
        for path in temporary.rglob("*.toml"):
            contents = path.read_text()
            assert "taildesk-mode" not in contents, "非法参数修改了已记住的模式"
            assert secret not in contents, "非法参数保存了配对密码"
        print(f"PASS {len(cases)} native CLI rejection cases; no mode or secret persisted", flush=True)
        probe = subprocess.run(
            [str(test_binary), "tailcat::live_tests::live_controller_does_not_start_host",
             "--ignored", "--exact", "--nocapture", "--test-threads=1"],
            cwd=repository, env=dict(environment, TAILDESK_CONTROLLER_PROBE="1"),
            capture_output=True, text=True, timeout=15,
        )
        assert probe.returncode == 0 and "LIVE_CONTROLLER_ONLY_OK" in probe.stdout, "控制模式意外启动了被控服务"
        print("PASS live controller startup creates no host IPC or server key", flush=True)
        role_probe = subprocess.run(
            [str(test_binary), "tailcat::live_tests::live_host_role_switch",
             "--ignored", "--exact", "--nocapture", "--test-threads=1"],
            cwd=repository, env=dict(environment, TAILDESK_CONTROLLER_PROBE="1"),
            capture_output=True, text=True, timeout=90,
        )
        assert role_probe.returncode == 0 and "LIVE_ROLE_LIFECYCLE_OK" in role_probe.stdout, (
            "被控服务角色切换检查失败\n" + role_probe.stdout[-1500:] + role_probe.stderr[-1500:]
        )
        timings = [line[line.index("LIVE_ROLE_RECONNECT_OK"):] for line in role_probe.stdout.splitlines()
                   if "LIVE_ROLE_RECONNECT_OK" in line]
        assert len(timings) == 3, "未完成三轮立即重连检查"
        for timing in timings:
            print(timing, flush=True)
        print("PASS immediate reconnects close old ingress and tunnels; address and key stay stable; stale snapshots cannot resume hosting", flush=True)


if __name__ == "__main__":
    main()
