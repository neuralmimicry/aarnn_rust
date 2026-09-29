"""Contract tests for the bounded local cluster launcher readiness check."""

from __future__ import annotations

import unittest

from wait_for_local_cluster import HierarchyContractError, inspect_status


class LocalClusterReadinessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workers = {
            "node_1": "127.0.0.1:50075",
            "node_2": "127.0.0.1:50087",
        }
        self.payload = {
            "networks": [{"network_id": "cluster_master"}],
            "nodes": [
                {
                    "node_id": "node_1",
                    "address": "http://127.0.0.1:50075",
                    "active_networks": ["cluster_master"],
                },
                {
                    "node_id": "node_2",
                    "address": "127.0.0.1:50087",
                    "active_networks": ["cluster_master"],
                },
            ],
        }

    def test_accepts_stable_nodes_hosting_one_cluster_brain(self) -> None:
        ready, summary = inspect_status(self.payload, "cluster_master", self.workers)
        self.assertTrue(ready)
        self.assertIn("node_1@127.0.0.1:50075", summary)
        self.assertIn("node_2@127.0.0.1:50087", summary)

    def test_waits_until_membership_and_hosted_brain_are_reported(self) -> None:
        self.payload["nodes"].pop()
        ready, explanation = inspect_status(self.payload, "cluster_master", self.workers)
        self.assertFalse(ready)
        self.assertIn("node_2 membership", explanation)

        self.payload["nodes"].append(
            {
                "node_id": "node_2",
                "address": "127.0.0.1:50087",
                "active_networks": [],
            }
        )
        ready, explanation = inspect_status(self.payload, "cluster_master", self.workers)
        self.assertFalse(ready)
        self.assertIn("node_2 hosted brain cluster_master", explanation)

    def test_rejects_worker_ids_reported_as_separate_brains(self) -> None:
        self.payload["networks"].append({"network_id": "node_1"})
        with self.assertRaisesRegex(HierarchyContractError, "separate brain IDs"):
            inspect_status(self.payload, "cluster_master", self.workers)

    def test_rejects_unexpected_brain_ids_for_single_brain_launcher(self) -> None:
        self.payload["networks"].append({"network_id": "unconfigured_brain"})
        with self.assertRaisesRegex(HierarchyContractError, "expected only brain"):
            inspect_status(self.payload, "cluster_master", self.workers)


if __name__ == "__main__":
    unittest.main()
