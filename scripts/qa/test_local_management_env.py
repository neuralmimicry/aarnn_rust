"""Regression checks for reusable mutual-TLS credentials in local launchers."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import local_management_env


class LocalManagementEnvironmentTests(unittest.TestCase):
    def _with_local_tls_environment(self, operation):
        empty_tls_environment = {name: "" for name in local_management_env.TLS_NAMES}
        with mock.patch.dict(os.environ, empty_tls_environment):
            return operation()

    def _assert_certificate_is_current(self, path: str) -> None:
        subprocess.run(
            ["openssl", "x509", "-in", path, "-noout", "-checkend", "3600"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def test_valid_local_identity_is_reused(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            runtime_root = Path(temporary_directory)
            first = self._with_local_tls_environment(
                lambda: local_management_env._ensure_tls(runtime_root)
            )
            first_certificate = Path(first["NM_GRPC_TLS_CERT"]).read_bytes()
            second = self._with_local_tls_environment(
                lambda: local_management_env._ensure_tls(runtime_root)
            )

            self.assertEqual(first, second)
            self.assertEqual(first_certificate, Path(second["NM_GRPC_TLS_CERT"]).read_bytes())
            self._assert_certificate_is_current(second["NM_GRPC_TLS_CERT"])

    def test_expired_local_identity_is_rotated(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            runtime_root = Path(temporary_directory)
            original = self._with_local_tls_environment(
                lambda: local_management_env._ensure_tls(runtime_root)
            )
            old_certificate = Path(original["NM_GRPC_TLS_CERT"]).read_bytes()
            real_run = subprocess.run

            def report_expired_certificate(command, *args, **kwargs):
                # Model OpenSSL rejecting both cached certificates by expiry,
                # while allowing the real commands that issue their replacements.
                if command[:2] == ["openssl", "x509"] and "-checkend" in command:
                    if kwargs.get("check"):
                        raise subprocess.CalledProcessError(1, command)
                    return subprocess.CompletedProcess(command, returncode=1)
                return real_run(command, *args, **kwargs)

            with mock.patch.object(
                local_management_env.subprocess,
                "run",
                side_effect=report_expired_certificate,
            ):
                renewed = self._with_local_tls_environment(
                    lambda: local_management_env._ensure_tls(runtime_root)
                )

            self.assertNotEqual(old_certificate, Path(renewed["NM_GRPC_TLS_CERT"]).read_bytes())
            self._assert_certificate_is_current(renewed["NM_GRPC_TLS_CA"])
            self._assert_certificate_is_current(renewed["NM_GRPC_TLS_CERT"])
            subprocess.run(
                [
                    "openssl",
                    "verify",
                    "-CAfile",
                    renewed["NM_GRPC_TLS_CA"],
                    renewed["NM_GRPC_TLS_CERT"],
                ],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )


if __name__ == "__main__":
    unittest.main()
