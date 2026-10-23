import json
from pathlib import Path
import subprocess
import sys
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


class ModelStructureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run([sys.executable, str(ROOT / "tools" / "generate.py")], check=True)
        cls.tree = ET.parse(ROOT / "model" / "ugot_mecanum.xml")
        cls.root = cls.tree.getroot()
        cls.config = json.loads((ROOT / "config" / "ugot_mecanum.json").read_text())

    def test_four_drive_actuators(self):
        self.assertEqual(len(self.root.findall("./actuator/motor")), 4)

    def test_passive_rollers(self):
        expected = 4 * self.config["dimensions"]["roller_count"]["value"]
        rollers = [g for g in self.root.findall(".//geom") if "_roller_" in g.get("name", "")]
        self.assertEqual(len(rollers), expected)

    def test_all_estimates_are_labelled(self):
        for section in ("dimensions", "mass", "drive"):
            for name, datum in self.config[section].items():
                self.assertIn("status", datum, f"{section}.{name} missing status")
                self.assertIn("value", datum, f"{section}.{name} missing value")

    def test_usd_has_articulation_and_joints(self):
        text = (ROOT / "model" / "ugot_mecanum.usda").read_text()
        self.assertIn("PhysicsArticulationRootAPI", text)
        self.assertEqual(text.count("def PhysicsRevoluteJoint"), 4)

    def test_finite_platform_replaces_infinite_plane(self):
        platform = self.root.find("./worldbody/geom[@name='platform']")
        self.assertIsNotNone(platform)
        self.assertEqual(platform.get("type"), "box")
        self.assertIsNone(self.root.find("./worldbody/geom[@type='plane']"))


if __name__ == "__main__":
    unittest.main()
