#!/usr/bin/env python3
"""Provision the local management_v1 environment used by developer launchers.

The complete Cargo feature graph enables the authenticated management service.
This helper supplies only loopback development credentials and an ephemeral
local mTLS identity; production launchers must provide their own values.
"""

from __future__ import annotations

import argparse
import os
import secrets
import shlex
import subprocess
from pathlib import Path


TLS_NAMES = ("NM_GRPC_TLS_CERT", "NM_GRPC_TLS_KEY", "NM_GRPC_TLS_CA")
ENV_NAMES = (
    "NM_MANAGEMENT_BEARER_TOKEN",
    "NM_MANAGEMENT_PRINCIPAL",
    "NM_MANAGEMENT_PRINCIPALS",
    "NM_MANAGEMENT_STATE_PATH",
    *TLS_NAMES,
    "NM_GRPC_TLS_DOMAIN",
)


def _run_openssl(args: list[str]) -> None:
    try:
        subprocess.run(
            ["openssl", *args],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except FileNotFoundError as exc:
        raise RuntimeError("openssl is required for the local management TLS identity") from exc
    except subprocess.CalledProcessError as exc:
        raise RuntimeError("openssl could not create the local management TLS identity") from exc


def _ensure_tls(runtime_root: Path) -> dict[str, str]:
    values = {name: os.environ.get(name, "") for name in TLS_NAMES}
    present = [bool(values[name]) for name in TLS_NAMES]
    if all(present):
        values["NM_GRPC_TLS_DOMAIN"] = os.environ.get("NM_GRPC_TLS_DOMAIN", "localhost")
        return values
    if any(present):
        raise RuntimeError(
            "NM_GRPC_TLS_CERT, NM_GRPC_TLS_KEY and NM_GRPC_TLS_CA must be configured together"
        )

    tls_dir = runtime_root / "management-tls"
    tls_dir.mkdir(parents=True, exist_ok=True)
    tls_dir.chmod(0o700)
    ca_key = tls_dir / "ca.key"
    ca_crt = tls_dir / "ca.crt"
    dev_key = tls_dir / "dev.key"
    dev_crt = tls_dir / "dev.crt"
    dev_csr = tls_dir / "dev.csr"
    extensions = tls_dir / "dev-extensions.cnf"

    if not all(path.is_file() and path.stat().st_size > 0 for path in (ca_key, ca_crt, dev_key, dev_crt)):
        _run_openssl(
            [
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(ca_key),
                "-out",
                str(ca_crt),
                "-days",
                "1",
                "-subj",
                "/CN=AARNN local launcher CA",
                "-addext",
                "basicConstraints=critical,CA:TRUE,pathlen:1",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ]
        )
        _run_openssl(
            [
                "req",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(dev_key),
                "-out",
                str(dev_csr),
                "-subj",
                "/CN=localhost",
            ]
        )
        extensions.write_text(
            "basicConstraints=critical,CA:FALSE\n"
            "keyUsage=critical,digitalSignature,keyEncipherment\n"
            "extendedKeyUsage=serverAuth,clientAuth\n"
            "subjectAltName=DNS:localhost,IP:127.0.0.1\n",
            encoding="utf-8",
        )
        _run_openssl(
            [
                "x509",
                "-req",
                "-in",
                str(dev_csr),
                "-CA",
                str(ca_crt),
                "-CAkey",
                str(ca_key),
                "-CAcreateserial",
                "-out",
                str(dev_crt),
                "-days",
                "1",
                "-sha256",
                "-extfile",
                str(extensions),
            ]
        )

    for path in (ca_key, dev_key):
        path.chmod(0o600)
    return {
        "NM_GRPC_TLS_CERT": str(dev_crt),
        "NM_GRPC_TLS_KEY": str(dev_key),
        "NM_GRPC_TLS_CA": str(ca_crt),
        "NM_GRPC_TLS_DOMAIN": os.environ.get("NM_GRPC_TLS_DOMAIN", "localhost"),
    }


def prepare(runtime_root: Path) -> dict[str, str]:
    runtime_root.mkdir(parents=True, exist_ok=True)
    values = {
        "NM_MANAGEMENT_BEARER_TOKEN": os.environ.get(
            "NM_MANAGEMENT_BEARER_TOKEN", secrets.token_hex(32)
        ),
        "NM_MANAGEMENT_PRINCIPAL": os.environ.get(
            "NM_MANAGEMENT_PRINCIPAL", "local-launcher"
        ),
    }
    values["NM_MANAGEMENT_PRINCIPALS"] = os.environ.get(
        "NM_MANAGEMENT_PRINCIPALS", values["NM_MANAGEMENT_PRINCIPAL"]
    )
    values["NM_MANAGEMENT_STATE_PATH"] = os.environ.get(
        "NM_MANAGEMENT_STATE_PATH", str(runtime_root / "management-state.json")
    )
    Path(values["NM_MANAGEMENT_STATE_PATH"]).parent.mkdir(parents=True, exist_ok=True)
    values.update(_ensure_tls(runtime_root))
    return values


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--shell", action="store_true")
    args = parser.parse_args()
    try:
        values = prepare(args.runtime_root)
    except RuntimeError as exc:
        parser.error(str(exc))
    if args.shell:
        for name in ENV_NAMES:
            print(f"export {name}={shlex.quote(values[name])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
