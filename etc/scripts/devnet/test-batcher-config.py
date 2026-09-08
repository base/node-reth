#!/usr/bin/env python3
"""Render both batchers in every devnet topology; requires Docker Compose, Bake, and just."""

import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[3]
COMPOSE = ["docker", "compose", "--env-file", "etc/docker/devnet-env"]
BAKE = ["docker", "buildx", "bake", "-f", "etc/docker/docker-bake.hcl"]
JUST = ["just", "--justfile", "etc/docker/Justfile"]


def run(args, env, check=True):
    return subprocess.run(args, cwd=ROOT, env=env, text=True, capture_output=True, check=check)


def option(args, name):
    for i, arg in enumerate(args):
        if arg == name or arg.startswith(name + "="):
            return arg.split("=", 1)[1] if "=" in arg else args[i + 1]
    raise AssertionError(f"Missing option: {name}")


def main():
    env = dict(os.environ)
    for name in ("BATCHER_IMPL", "DEVNET_TARGETS", "INGRESS_TARGETS"):
        env.pop(name, None)
    # Non-default ports exercise interpolation in the templates and HA overlay.
    env.update(L1_HTTP_PORT="14545", L2_BUILDER_HTTP_PORT="17545",
               L2_BUILDER_CL_RPC_PORT="17549", CONDUCTOR0_RPC_PORT="16545",
               BATCHER_METRICS_PORT="16060")

    for impl in (None, "op-batcher", "base-batcher"):
        if impl is not None:
            env["BATCHER_IMPL"] = impl
        is_op = impl != "base-batcher"
        for mode in ("single", "ha", "anvil-l1", "ingress", "profiling"):
            args = COMPOSE + ["-f", "etc/docker/docker-compose.yml"]
            if mode != "single":
                overlay = "ha" if mode == "profiling" else mode
                args += ["-f", f"etc/docker/docker-compose.{overlay}.yml"]
            if mode == "profiling":
                args += ["--profile", "profiling"]
            services = json.loads(run(args + ["config", "--format", "json"], env).stdout)["services"]
            batcher = services["base-batcher"]
            assert "op-batcher" not in services  # Never launch a second batcher.
            assert batcher["image"] == ("op-batcher:local" if is_op else "base:local")
            build = batcher["build"]
            assert build["context"] == str(ROOT)
            assert build["dockerfile"] == "etc/docker/" + (
                "Dockerfile.op-batcher" if is_op else "Dockerfile.rust-services")
            assert build.get("target") == (None if is_op else "base")
            assert batcher.get("entrypoint") == (None if is_op else ["/app/base"])
            command = batcher["command"]
            assert (command[0] == "batcher") == (not is_op)
            assert option(command, "--l1-eth-rpc" if is_op else "--l1-rpc-url") == "http://l1-el:14545"
            assert option(command, "--l2-eth-rpc" if is_op else "--l2-rpc-url") == "http://base-builder:17545"
            assert option(command, "--poll-interval") == ("1s" if is_op else "1")
            assert option(command, "--metrics.port") == "16060"
            rpc_env = "OP_BATCHER_ROLLUP_RPC" if is_op else "BASE_BATCHER_ROLLUP_RPC_URL"
            ha = mode in ("ha", "profiling")
            assert batcher["environment"][rpc_env] == (
                "http://op-conductor-0:16545" if ha else "http://base-builder:17549")
            assert ("setup-conductor" in batcher["depends_on"]) == ha
            if is_op:
                for name, value in (("batch-type", "0"), ("data-availability-type", "blobs"),
                                    ("compression-algo", "brotli"), ("txmgr.cell-proof-time", "0")):
                    assert option(command, "--" + name) == value
            print(f"PASS {impl or 'default'} / {mode}")

        for group in ("devnet", "ingress"):
            targets = json.loads(run(BAKE + [group, "--print"], env).stdout)["target"]
            expected = {"base"} | ({"op-batcher"} if is_op else set())
            if group == "ingress":
                expected |= {"ingress-rpc", "audit-archiver"}
            assert set(targets) == expected
        assert run(JUST + ["--evaluate", "BATCHER_IMPL"], env).stdout.strip() == (impl or "op-batcher")

    # File configuration and shell override must select the same implementation in just.
    with tempfile.NamedTemporaryFile(mode="w") as dotenv:
        dotenv.write("BATCHER_IMPL=base-batcher\n")
        dotenv.flush()
        env.pop("BATCHER_IMPL")
        args = JUST + ["--dotenv-path", dotenv.name, "--evaluate", "BATCHER_IMPL"]
        assert run(args, env).stdout.strip() == "base-batcher"
        env["BATCHER_IMPL"] = "op-batcher"
        assert run(args, env).stdout.strip() == "op-batcher"

    env["BATCHER_IMPL"] = "typo"
    for args in (COMPOSE + ["-f", "etc/docker/docker-compose.yml", "config", "-q"],
                 BAKE + ["devnet", "--print"], JUST + ["--evaluate", "BATCHER_IMPL"]):
        assert run(args, env, check=False).returncode != 0
    print("PASS Bake targets, dotenv/shell selection, and invalid implementation rejection")


if __name__ == "__main__":
    main()
