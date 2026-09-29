"""Regression checks for the run_examples audio I/O contract helper."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from prepare_audio_io_contract import prepare_audio_io_contract


class AudioIoContractTests(unittest.TestCase):
    def test_zero_width_config_and_snapshot_receive_requested_audio_width(self) -> None:
        contract, seeded = prepare_audio_io_contract(
            {"num_sensory_neurons": 0, "num_output_neurons": 96},
            {"net": {"num_sensory_neurons": 0}},
            64,
        )

        self.assertTrue(seeded)
        self.assertEqual(contract["num_sensory_neurons"], 64)
        self.assertEqual(contract["num_output_neurons"], 96)

    def test_positive_configured_width_is_preserved(self) -> None:
        contract, seeded = prepare_audio_io_contract(
            {"num_sensory_neurons": 1},
            {"net": {"num_sensory_neurons": 0}},
            64,
        )

        self.assertFalse(seeded)
        self.assertEqual(contract["num_sensory_neurons"], 1)

    def test_positive_snapshot_width_is_preserved_when_config_is_unspecified(self) -> None:
        contract, seeded = prepare_audio_io_contract(
            {"num_sensory_neurons": 0},
            {"net": {"num_sensory_neurons": 8}},
            64,
        )

        self.assertFalse(seeded)
        self.assertEqual(contract["num_sensory_neurons"], 0)

    def test_missing_config_uses_snapshot_network_config_as_contract_base(self) -> None:
        contract, seeded = prepare_audio_io_contract(
            None,
            {"net": {"num_sensory_neurons": 0, "num_hidden_layers": 2}},
            32,
        )

        self.assertTrue(seeded)
        self.assertEqual(contract["num_sensory_neurons"], 32)
        self.assertEqual(contract["num_hidden_layers"], 2)


if __name__ == "__main__":
    unittest.main()
