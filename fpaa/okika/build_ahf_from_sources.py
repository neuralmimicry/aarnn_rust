#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
import textwrap
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Sequence


@dataclass(frozen=True)
class KernelSpec:
    kernel_id: str
    xcos_script: Path
    okika_manifest: Path
    pika_wrapper: Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _fpaa_root(repo_root: Path) -> Path:
    return repo_root / "fpaa"


def _okika_root(repo_root: Path) -> Path:
    return _fpaa_root(repo_root) / "okika"


def _algorithms_path(repo_root: Path) -> Path:
    return _fpaa_root(repo_root) / "algorithms.json"


def _run(
    cmd: Sequence[str],
    *,
    cwd: Path | None = None,
    env: Dict[str, str] | None = None,
    dry_run: bool = False,
) -> None:
    printable = " ".join(shlex.quote(part) for part in cmd)
    if cwd is not None:
        print(f"[cmd] (cd {cwd}) {printable}")
    else:
        print(f"[cmd] {printable}")
    if dry_run:
        return
    subprocess.run(
        list(cmd),
        cwd=str(cwd) if cwd is not None else None,
        env=env,
        check=True,
    )


def _sudo_prefix() -> List[str]:
    if os.geteuid() == 0:
        return []
    return ["sudo"]


def _apt_install(packages: Iterable[str], *, dry_run: bool) -> None:
    pkg_list = list(packages)
    if not pkg_list:
        return
    _run(
        _sudo_prefix() + ["apt-get", "install", "-y"] + pkg_list,
        dry_run=dry_run,
    )


def _bootstrap_tools(args: argparse.Namespace) -> int:
    if shutil.which("apt-get") is None:
        raise SystemExit("bootstrap-tools currently supports Debian/Ubuntu hosts (apt-get) only.")

    _run(_sudo_prefix() + ["apt-get", "update"], dry_run=args.dry_run)

    base_packages = [
        "ca-certificates",
        "curl",
        "jq",
        "python3",
        "python3-pip",
    ]
    _apt_install(base_packages, dry_run=args.dry_run)

    if args.with_scilab:
        try:
            _apt_install(["scilab-cli"], dry_run=args.dry_run)
        except subprocess.CalledProcessError:
            print("scilab-cli install failed, retrying with scilab package...")
            _apt_install(["scilab"], dry_run=args.dry_run)

    if args.with_wine:
        _apt_install(["wine64", "winetricks", "xvfb"], dry_run=args.dry_run)

    if args.vendor_tool_url:
        parsed = urllib.parse.urlparse(args.vendor_tool_url)
        guessed_name = Path(parsed.path).name.strip() or "fpaa_vendor_tool.bin"
        file_name = args.vendor_tool_filename.strip() if args.vendor_tool_filename else guessed_name
        download_dir = Path(args.vendor_download_dir).expanduser().resolve()
        target_path = download_dir / file_name
        print(f"[info] downloading vendor tool: {args.vendor_tool_url} -> {target_path}")
        if not args.dry_run:
            download_dir.mkdir(parents=True, exist_ok=True)
            urllib.request.urlretrieve(args.vendor_tool_url, target_path)
            if args.vendor_tool_sha256:
                digest = hashlib.sha256(target_path.read_bytes()).hexdigest()
                expected = args.vendor_tool_sha256.strip().lower()
                if digest.lower() != expected:
                    raise SystemExit(
                        f"sha256 mismatch for {target_path}: expected {expected}, got {digest}"
                    )
        if args.vendor_tool_sha256:
            print("[info] vendor tool sha256 verified")

    print("[ok] tool bootstrap completed")
    return 0


def _load_supported_kernels(repo_root: Path) -> List[KernelSpec]:
    algorithms_file = _algorithms_path(repo_root)
    payload = json.loads(algorithms_file.read_text(encoding="utf-8"))
    supported = payload.get("supported", [])
    if not isinstance(supported, list):
        raise SystemExit(f"invalid algorithms.json: 'supported' must be an array ({algorithms_file})")

    kernels: List[KernelSpec] = []
    for entry in supported:
        if not isinstance(entry, dict):
            continue
        kernel_id = str(entry.get("id", "")).strip()
        if not kernel_id:
            continue
        xcos = _fpaa_root(repo_root) / str(entry.get("xcos_script", "")).strip()
        manifest = _fpaa_root(repo_root) / str(entry.get("okika_manifest", "")).strip()
        wrapper = _fpaa_root(repo_root) / str(entry.get("pika_wrapper", "")).strip()
        kernels.append(
            KernelSpec(
                kernel_id=kernel_id,
                xcos_script=xcos,
                okika_manifest=manifest,
                pika_wrapper=wrapper,
            )
        )
    return kernels


def _load_manifest_expected_ahf(path: Path) -> str:
    payload = json.loads(path.read_text(encoding="utf-8"))
    expected = str(payload.get("okika_design", {}).get("expected_ahf", "")).strip()
    if not expected:
        raise SystemExit(f"manifest missing okika_design.expected_ahf: {path}")
    return expected


def _validate_ahf(path: Path) -> int:
    byte_count = 0
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        token = raw.strip()
        if not token:
            continue
        if len(token) > 2:
            raise SystemExit(f"{path}:{lineno}: expected one hex byte per line, got {token!r}")
        try:
            value = int(token, 16)
        except ValueError as exc:
            raise SystemExit(f"{path}:{lineno}: invalid hex byte {token!r}") from exc
        if value < 0 or value > 0xFF:
            raise SystemExit(f"{path}:{lineno}: byte out of range {token!r}")
        byte_count += 1
    if byte_count == 0:
        raise SystemExit(f"{path}: empty .ahf")
    return byte_count


def _resolve_kernel_selection(
    kernels: Sequence[KernelSpec],
    selectors: Sequence[str],
) -> List[KernelSpec]:
    if not selectors:
        return list(kernels)

    selector_set = {item.strip() for item in selectors if item.strip()}
    if "all" in selector_set:
        return list(kernels)

    by_id: Dict[str, KernelSpec] = {k.kernel_id: k for k in kernels}
    by_manifest_stem: Dict[str, KernelSpec] = {
        k.okika_manifest.name.replace(".okika.json", ""): k for k in kernels
    }

    selected: List[KernelSpec] = []
    seen: set[str] = set()
    for raw in selectors:
        selector = raw.strip()
        if not selector:
            continue
        kernel = by_id.get(selector) or by_manifest_stem.get(selector)
        if kernel is None:
            supported = ", ".join(sorted(by_id))
            raise SystemExit(f"unknown kernel selector '{selector}'. supported: {supported}")
        if kernel.kernel_id in seen:
            continue
        seen.add(kernel.kernel_id)
        selected.append(kernel)
    return selected


def _find_scilab_bin(explicit: str | None) -> str:
    candidates = [explicit] if explicit else []
    env_bin = os.getenv("SCILAB_BIN")
    if env_bin:
        candidates.append(env_bin)
    candidates.extend(["scilab-cli", "scilab"])
    for candidate in candidates:
        if not candidate:
            continue
        resolved = shutil.which(candidate) if Path(candidate).name == candidate else candidate
        if resolved and Path(resolved).exists():
            return resolved
    raise SystemExit(
        "no Scilab binary found. install with bootstrap-tools or pass --scilab-bin /path/to/scilab-cli"
    )


def _run_xcos_script(scilab_bin: str, script_path: Path, *, dry_run: bool) -> None:
    _run(
        [scilab_bin, "-nb", "-nwni", "-f", str(script_path)],
        cwd=script_path.parent,
        dry_run=dry_run,
    )


def _build(args: argparse.Namespace) -> int:
    repo_root = _repo_root()
    kernels = _load_supported_kernels(repo_root)
    selected = _resolve_kernel_selection(kernels, args.kernel)
    if not selected:
        raise SystemExit("no kernels selected")

    scilab_bin = None
    if args.run_xcos:
        scilab_bin = _find_scilab_bin(args.scilab_bin)
        print(f"[info] using Scilab binary: {scilab_bin}")

    copy_from = Path(args.copy_from).expanduser().resolve() if args.copy_from else None
    if copy_from and not copy_from.is_dir():
        raise SystemExit(f"--copy-from directory does not exist: {copy_from}")

    export_script = Path(args.export_script).expanduser().resolve() if args.export_script else None
    if export_script and not export_script.exists():
        raise SystemExit(f"--export-script does not exist: {export_script}")

    okika_root = _okika_root(repo_root)
    failures: List[str] = []

    for kernel in selected:
        print(f"\n== kernel: {kernel.kernel_id} ==")
        try:
            if not kernel.okika_manifest.exists():
                raise SystemExit(f"missing manifest: {kernel.okika_manifest}")
            if not kernel.xcos_script.exists():
                raise SystemExit(f"missing xcos script: {kernel.xcos_script}")
            if args.program_dry_run and not kernel.pika_wrapper.exists():
                raise SystemExit(f"missing pika wrapper: {kernel.pika_wrapper}")

            expected_ahf = _load_manifest_expected_ahf(kernel.okika_manifest)
            output_ahf = okika_root / expected_ahf
            print(f"[info] manifest: {kernel.okika_manifest}")
            print(f"[info] xcos:     {kernel.xcos_script}")
            print(f"[info] ahf:      {output_ahf}")

            if args.run_xcos:
                assert scilab_bin is not None
                _run_xcos_script(scilab_bin, kernel.xcos_script, dry_run=args.dry_run)

            if not args.validate_only:
                generated = False
                if copy_from is not None:
                    source_ahf = copy_from / expected_ahf
                    if not source_ahf.exists():
                        raise SystemExit(f"copy source missing: {source_ahf}")
                    if not args.dry_run:
                        shutil.copy2(source_ahf, output_ahf)
                    print(f"[info] copied {source_ahf} -> {output_ahf}")
                    generated = True
                elif export_script is not None:
                    _run(
                        [
                            str(export_script),
                            "--kernel-id",
                            kernel.kernel_id,
                            "--manifest",
                            str(kernel.okika_manifest),
                            "--xcos",
                            str(kernel.xcos_script),
                            "--output",
                            str(output_ahf),
                        ],
                        cwd=repo_root,
                        dry_run=args.dry_run,
                    )
                    generated = True
                elif args.export_command_template:
                    substitutions = {
                        "kernel_id": kernel.kernel_id,
                        "manifest": str(kernel.okika_manifest),
                        "manifest_name": kernel.okika_manifest.name,
                        "manifest_dir": str(kernel.okika_manifest.parent),
                        "xcos": str(kernel.xcos_script),
                        "xcos_name": kernel.xcos_script.name,
                        "xcos_dir": str(kernel.xcos_script.parent),
                        "output": str(output_ahf),
                        "expected_ahf": expected_ahf,
                        "okika_dir": str(okika_root),
                        "kernel_id_q": shlex.quote(kernel.kernel_id),
                        "manifest_q": shlex.quote(str(kernel.okika_manifest)),
                        "xcos_q": shlex.quote(str(kernel.xcos_script)),
                        "output_q": shlex.quote(str(output_ahf)),
                        "okika_dir_q": shlex.quote(str(okika_root)),
                    }
                    shell_cmd = args.export_command_template.format(**substitutions)
                    _run(["bash", "-lc", shell_cmd], cwd=repo_root, dry_run=args.dry_run)
                    generated = True
                elif output_ahf.exists():
                    print("[info] existing .ahf present; skipping generation")
                else:
                    raise SystemExit(
                        "no generation method selected. use one of "
                        "--copy-from, --export-script, or --export-command-template"
                    )

                if generated and args.dry_run:
                    print("[dry-run] generation command executed in dry-run mode")

            if not output_ahf.exists() and not args.dry_run:
                raise SystemExit(f"expected .ahf not found after build: {output_ahf}")

            if output_ahf.exists():
                byte_count = _validate_ahf(output_ahf)
                print(f"[ok] validated .ahf bytes: {byte_count}")
            else:
                print("[dry-run] validation skipped because .ahf file is not written in dry-run mode")

            if args.program_dry_run:
                _run([sys.executable, str(kernel.pika_wrapper), "--dry-run"], cwd=okika_root, dry_run=args.dry_run)
                print("[ok] wrapper dry-run completed")

        except Exception as exc:  # noqa: BLE001
            failures.append(f"{kernel.kernel_id}: {exc}")
            print(f"[error] {kernel.kernel_id}: {exc}", file=sys.stderr)
            if not args.keep_going:
                break

    if failures:
        print("\n[summary] failures:")
        for item in failures:
            print(f"  - {item}")
        return 1

    print("\n[summary] all selected kernels processed successfully")
    return 0


def _list_kernels() -> int:
    kernels = _load_supported_kernels(_repo_root())
    for kernel in kernels:
        print(
            f"{kernel.kernel_id}\n"
            f"  manifest: {kernel.okika_manifest}\n"
            f"  xcos:     {kernel.xcos_script}\n"
            f"  wrapper:  {kernel.pika_wrapper}"
        )
    return 0


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Bootstrap FPAA toolchain and build/validate .ahf files for AARNN kernels."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    bootstrap = subparsers.add_parser(
        "bootstrap-tools",
        help="Install/download tooling required for .ahf generation.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent(
            """\
            Examples:
              ./build_ahf_from_sources.py bootstrap-tools
              ./build_ahf_from_sources.py bootstrap-tools --with-wine \\
                --vendor-tool-url https://example.com/vendor-installer.exe \\
                --vendor-tool-sha256 <sha256>
            """
        ),
    )
    bootstrap.add_argument("--dry-run", action="store_true", help="Print actions without modifying the host.")
    bootstrap.add_argument(
        "--with-scilab",
        dest="with_scilab",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Install Scilab CLI package.",
    )
    bootstrap.add_argument(
        "--with-wine",
        dest="with_wine",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Install Wine + xvfb for vendor GUI tooling under automation wrappers.",
    )
    bootstrap.add_argument(
        "--vendor-tool-url",
        default="",
        help="Optional URL for proprietary FPAA design/export installer.",
    )
    bootstrap.add_argument(
        "--vendor-tool-filename",
        default="",
        help="Override downloaded installer filename.",
    )
    bootstrap.add_argument(
        "--vendor-tool-sha256",
        default="",
        help="Optional sha256 for downloaded installer integrity check.",
    )
    bootstrap.add_argument(
        "--vendor-download-dir",
        default=str(_okika_root(_repo_root()) / ".tooling"),
        help="Directory for downloaded vendor tool artefacts.",
    )
    bootstrap.set_defaults(func=_bootstrap_tools)

    build = subparsers.add_parser(
        "build",
        help="Generate and validate .ahf files from Okika/Xcos sources.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent(
            """\
            Examples:
              ./build_ahf_from_sources.py build --kernel synaptic_filter --validate-only

              ./build_ahf_from_sources.py build --kernel all \\
                --export-command-template 'vendor_cli --manifest {manifest_q} --xcos {xcos_q} --output {output_q}'

              ./build_ahf_from_sources.py build --kernel all \\
                --copy-from /opt/fpaa/exports --program-dry-run
            """
        ),
    )
    build.add_argument(
        "--kernel",
        action="append",
        default=[],
        help="Kernel selector (kernel id or manifest stem). Repeatable. Default: all.",
    )
    build.add_argument(
        "--run-xcos",
        dest="run_xcos",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Execute Scilab scripts before .ahf generation.",
    )
    build.add_argument(
        "--scilab-bin",
        default="",
        help="Explicit Scilab binary path (otherwise SCILAB_BIN/scilab-cli/scilab).",
    )
    build.add_argument(
        "--validate-only",
        action="store_true",
        help="Do not generate .ahf; only validate presence and format.",
    )
    build.add_argument(
        "--copy-from",
        default="",
        help="Copy expected .ahf files from this directory into fpaa/okika.",
    )
    build.add_argument(
        "--export-script",
        default="",
        help=(
            "Executable to invoke for each kernel export. It receives "
            "--kernel-id, --manifest, --xcos, --output."
        ),
    )
    build.add_argument(
        "--export-command-template",
        default="",
        help=(
            "Shell command template for per-kernel export. Placeholders: "
            "{kernel_id}, {manifest}, {xcos}, {output}, and *_q quoted variants."
        ),
    )
    build.add_argument(
        "--program-dry-run",
        action="store_true",
        help="After .ahf build, run corresponding program_*.py wrapper with --dry-run.",
    )
    build.add_argument("--keep-going", action="store_true", help="Continue other kernels after a failure.")
    build.add_argument("--dry-run", action="store_true", help="Print commands only.")
    build.set_defaults(func=_build)

    list_cmd = subparsers.add_parser("list-kernels", help="List supported kernel ids and source paths.")
    list_cmd.set_defaults(func=lambda _args: _list_kernels())

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
