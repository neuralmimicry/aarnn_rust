#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

PINS = {
    "FPAA_RESET_B": 26,
    "FPAA_CFGFLG_B": 23,
    "FPAA_ACTIVATE": 24,
    "FPAA_ERR_B": 25,
    "FPAA_ACLK_REF": 4,
    "FPAA_LCL_ACLK_EN": 14,
    "FPAA_ACLK_SEL0": 5,
    "FPAA_ACLK_SEL1": 6,
    "FPAA_CE0": 8,
    "FPAA0_IO5P": 16,
    "FPAA0_IO5N": 17,
    "FPAA1_IO5P": 18,
    "FPAA1_IO5N": 19,
    "FPAA2_IO5P": 20,
    "FPAA2_IO5N": 21,
    "FPAA3_IO5P": 12,
    "FPAA3_IO5N": 13,
}


def _load_manifest(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _expected_ahf_path(manifest_path: Path, manifest: Dict[str, Any], ahf_override: Optional[Path]) -> Path:
    if ahf_override is not None:
        return ahf_override
    expected = manifest["okika_design"]["expected_ahf"]
    return manifest_path.with_name(expected)


def _load_ahf_bytes(path: Path) -> List[int]:
    data: List[int] = []
    with path.open("r", encoding="utf-8") as handle:
        for lineno, raw_line in enumerate(handle, start=1):
            token = raw_line.strip()
            if not token:
                continue
            if len(token) > 2:
                raise ValueError(f"{path}:{lineno}: expected one byte per line, got {token!r}")
            try:
                byte = int(token, 16)
            except ValueError as exc:
                raise ValueError(f"{path}:{lineno}: invalid hex byte {token!r}") from exc
            if not 0 <= byte <= 0xFF:
                raise ValueError(f"{path}:{lineno}: byte out of range {token!r}")
            data.append(byte)
    if not data:
        raise ValueError(f"{path}: no bytes found")
    return data


def _fnv1a64(data: Iterable[int]) -> int:
    value = 0xCBF29CE484222325
    for byte in data:
        value ^= int(byte) & 0xFF
        value = (value * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value


def _ahf_fingerprint(data: List[int]) -> str:
    return f"fnv1a64:{_fnv1a64(data):016x}:{len(data)}"


def _print_manifest_summary(manifest_path: Path, manifest: Dict[str, Any], ahf_path: Path, ahf_bytes: Optional[Iterable[int]]) -> None:
    design = manifest["okika_design"]
    print(f"manifest: {manifest_path}")
    print(f"kernel:   {manifest['title']}")
    print(f"class:    {manifest['analog_class']}")
    print(f"ahf:      {ahf_path}")
    if ahf_bytes is None:
        print("bytes:    missing")
    else:
        print(f"bytes:    {len(list(ahf_bytes))}")
    print(f"spi_hz:   {design['pika']['spi_hz']}")
    print(f"clock:    {design['pika']['clock_source']}")
    print(f"export:   {design['export_profile']['configuration_type']} / {design['export_profile']['container']}")


def _import_hardware_modules():
    try:
        import spidev  # type: ignore
        import RPi.GPIO as GPIO  # type: ignore
    except ImportError as exc:
        raise SystemExit(
            "spidev and RPi.GPIO are required on the Raspberry Pi host. "
            "Use --dry-run on non-Pi machines."
        ) from exc
    return spidev, GPIO


def _configure_gpio(GPIO) -> None:
    GPIO.setmode(GPIO.BCM)
    GPIO.setwarnings(False)
    GPIO.setup(PINS["FPAA_RESET_B"], GPIO.OUT, initial=GPIO.LOW)
    GPIO.setup(PINS["FPAA_CFGFLG_B"], GPIO.IN)
    GPIO.setup(PINS["FPAA_ACTIVATE"], GPIO.IN)
    GPIO.setup(PINS["FPAA_ERR_B"], GPIO.IN)
    GPIO.setup(PINS["FPAA_CE0"], GPIO.OUT, initial=GPIO.LOW)
    GPIO.setup(PINS["FPAA_LCL_ACLK_EN"], GPIO.OUT)
    GPIO.setup(PINS["FPAA_ACLK_SEL0"], GPIO.OUT, initial=GPIO.LOW)
    GPIO.setup(PINS["FPAA_ACLK_SEL1"], GPIO.OUT, initial=GPIO.LOW)
    for pin_name in [
        "FPAA0_IO5P",
        "FPAA0_IO5N",
        "FPAA1_IO5P",
        "FPAA1_IO5N",
        "FPAA2_IO5P",
        "FPAA2_IO5N",
        "FPAA3_IO5P",
        "FPAA3_IO5N",
    ]:
        GPIO.setup(PINS[pin_name], GPIO.IN)


def _program_bytes(data: List[int], spi_hz: int) -> None:
    spidev, GPIO = _import_hardware_modules()
    _configure_gpio(GPIO)
    spi = spidev.SpiDev()
    spi.open(0, 0)
    spi.mode = 0b00
    spi.no_cs = True
    spi.lsbfirst = False
    spi.bits_per_word = 8
    spi.max_speed_hz = spi_hz

    try:
        GPIO.output(PINS["FPAA_RESET_B"], 0)
        GPIO.output(PINS["FPAA_LCL_ACLK_EN"], 1)
        GPIO.output(PINS["FPAA_ACLK_SEL0"], 0)
        GPIO.output(PINS["FPAA_ACLK_SEL1"], 0)
        GPIO.output(PINS["FPAA_CE0"], 0)
        time.sleep(0.02)
        GPIO.output(PINS["FPAA_RESET_B"], 1)
        time.sleep(0.10)

        spi.xfer2([0])
        if GPIO.input(PINS["FPAA_ERR_B"]) == 0:
            print("warning: ERR_B is still low after dummy clocks; check ACLK and board wiring", file=sys.stderr)

        spi.xfer2(data)
        activate = GPIO.input(PINS["FPAA_ACTIVATE"])
        err_b = GPIO.input(PINS["FPAA_ERR_B"])
        cfgflg_b = GPIO.input(PINS["FPAA_CFGFLG_B"])
        print(f"programmed {len(data)} bytes")
        print(f"ACTIVATE={activate} ERR_B={err_b} CFGFLG_B={cfgflg_b}")
    finally:
        spi.close()
        GPIO.cleanup()


def _runtime_state_path(manifest_path: Path) -> Path:
    return manifest_path.resolve().parent.parent / "runtime_state.json"


def _load_runtime_state(path: Path) -> Dict[str, Any]:
    if not path.exists():
        return {}
    try:
        raw = path.read_text(encoding="utf-8")
        payload = json.loads(raw)
    except Exception as exc:
        print(f"warning: ignoring malformed runtime-state file {path}: {exc}", file=sys.stderr)
        return {}
    if not isinstance(payload, dict):
        print(f"warning: ignoring non-object runtime-state file {path}", file=sys.stderr)
        return {}
    return payload


def _write_runtime_state(manifest_path: Path, manifest: Dict[str, Any], ahf_path: Path, ahf_bytes: List[int]) -> None:
    state_path = _runtime_state_path(manifest_path)
    transport_id = "pihat_gpio_spi"
    existing = _load_runtime_state(state_path)
    existing_transport = str(existing.get("transport", "")).strip()
    existing_loaded = existing.get("loaded_kernels", [])
    if not isinstance(existing_loaded, list):
        existing_loaded = []
    merged_loaded: List[Dict[str, Any]] = []

    # Keep previously programmed kernels when they came from the same transport record.
    if existing_transport and existing_transport != transport_id and existing_loaded:
        print(
            "warning: runtime-state transport changed; replacing previous loaded-kernel records",
            file=sys.stderr,
        )
    else:
        for entry in existing_loaded:
            if not isinstance(entry, dict):
                continue
            kernel_id = str(entry.get("kernel_id", "")).strip()
            if not kernel_id or kernel_id == manifest["id"]:
                continue
            merged_loaded.append(entry)

    merged_loaded.append(
        {
            "kernel_id": manifest["id"],
            "manifest_path": str(manifest_path.resolve()),
            "ahf_path": str(ahf_path.resolve()),
            "ahf_fingerprint": _ahf_fingerprint(ahf_bytes),
        }
    )
    merged_loaded.sort(key=lambda item: str(item.get("kernel_id", "")))

    payload = {
        "schema_version": 1,
        "transport": transport_id,
        "updated_at_unix_s": int(time.time()),
        "loaded_kernels": merged_loaded,
    }
    state_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"runtime-state: {state_path}")


def program_pika_from_manifest(manifest_path: Path, ahf_override: Optional[Path] = None, dry_run: bool = False) -> int:
    manifest = _load_manifest(manifest_path)
    ahf_path = _expected_ahf_path(manifest_path, manifest, ahf_override)
    design = manifest["okika_design"]
    max_bytes = int(design["pika"].get("max_xfer_bytes", 4096))

    if ahf_path.exists():
        ahf_bytes = _load_ahf_bytes(ahf_path)
        if len(ahf_bytes) > max_bytes:
            raise SystemExit(
                f"AHF file has {len(ahf_bytes)} bytes, exceeding the configured xfer2 limit of {max_bytes}."
            )
    else:
        ahf_bytes = None

    _print_manifest_summary(manifest_path, manifest, ahf_path, ahf_bytes)

    if dry_run:
        if ahf_bytes is None:
            print("dry-run: manifest is valid; expected .ahf is not present yet")
        else:
            print("dry-run: manifest and .ahf look consistent")
        return 0

    if ahf_bytes is None:
        raise SystemExit(f"expected .ahf file not found: {ahf_path}")

    _program_bytes(ahf_bytes, int(design["pika"]["spi_hz"]))
    _write_runtime_state(manifest_path, manifest, ahf_path, ahf_bytes)
    return 0


def cli_for_fixed_manifest(manifest_path: Path) -> int:
    parser = argparse.ArgumentParser(description=f"Program Pi.Ka using {manifest_path.name}")
    parser.add_argument("--ahf", type=Path, default=None, help="Override the .ahf path")
    parser.add_argument("--dry-run", action="store_true", help="Validate manifest and optional .ahf without touching hardware")
    args = parser.parse_args()
    return program_pika_from_manifest(manifest_path, args.ahf, args.dry_run)


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Program a Pi.Ka board from an Okika manifest")
    parser.add_argument("manifest", type=Path, help="Path to a *.okika.json manifest")
    parser.add_argument("--ahf", type=Path, default=None, help="Override the .ahf path")
    parser.add_argument("--dry-run", action="store_true", help="Validate manifest and optional .ahf without touching hardware")
    args = parser.parse_args(argv)
    return program_pika_from_manifest(args.manifest.resolve(), args.ahf, args.dry_run)


if __name__ == "__main__":
    raise SystemExit(main())
