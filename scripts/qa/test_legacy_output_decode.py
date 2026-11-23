#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Offline vectors for the Unity/Unreal legacy-body brain output decoders.

Checks the generated OEM NAO engine tables are current, builds
scripts/qa/test_unreal_output_decode.cpp against the header-only
sim/unreal/Source/NmAerBridge/Public/NmOutputDecode.h, and (when a .NET SDK is
present) scripts/qa/test_unity_output_decode.cs against
sim/unity/Assets/NeuralMimicry/Runtime/NmOutputDecode.cs as C# 9. Neither
engine is required; build products stay in a temporary directory.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
QA = ROOT / 'scripts/qa'


class LegacyOutputDecode(unittest.TestCase):
    def test_nao_engine_tables_match_generated_record(self):
        # Unity/Unreal legacy NAO bodies freeze the OEM record (ranges, drives,
        # readout channels, decode_tau_s, wake); stale tables lose parity.
        result = subprocess.run(['python3', str(ROOT / 'scripts/generate_nao_engine_tables.py'), '--check'],
                                capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unreal_header_matches_reference_vectors(self):
        compiler = next((c for c in (os.environ.get('CXX'), 'c++', 'g++', 'clang++') if c and shutil.which(c)), None)
        if compiler is None:
            self.skipTest('no C++ compiler')
        with tempfile.TemporaryDirectory() as folder:
            binary = Path(folder) / 'test_unreal_output_decode'
            subprocess.run([compiler, '-std=c++17', '-O1', '-Wall', '-Wextra', '-Wshadow', '-Wconversion', '-Werror',
                            '-I', str(ROOT / 'sim/unreal/Source/NmAerBridge/Public'),
                            str(QA / 'test_unreal_output_decode.cpp'), '-o', str(binary)], check=True)
            result = subprocess.run([str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unity_decoder_matches_reference_vectors(self):
        dotnet = shutil.which('dotnet')
        if dotnet is None:
            self.skipTest('no .NET SDK')
        sdks = subprocess.run([dotnet, '--list-sdks'], capture_output=True, text=True).stdout.split()
        if not sdks:
            self.skipTest('no .NET SDK')
        framework = 'net' + '.'.join(sdks[0].split('.')[:2])
        with tempfile.TemporaryDirectory() as folder:
            project = Path(folder) / 'UnityOutputDecode.csproj'
            project.write_text(f'''<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>{framework}</TargetFramework>
    <LangVersion>9.0</LangVersion>
    <Nullable>disable</Nullable>
    <ImplicitUsings>disable</ImplicitUsings>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="{ROOT / 'sim/unity/Assets/NeuralMimicry/Runtime/NmOutputDecode.cs'}" />
    <Compile Include="{QA / 'test_unity_output_decode.cs'}" />
  </ItemGroup>
</Project>
''')
            env = dict(os.environ, DOTNET_CLI_TELEMETRY_OPTOUT='1', DOTNET_NOLOGO='1')
            result = subprocess.run([dotnet, 'run', '-c', 'Release', '--project', str(project)],
                                    capture_output=True, text=True, timeout=600, env=env, cwd=folder)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn('PASS', result.stdout)


if __name__ == '__main__':
    unittest.main()
