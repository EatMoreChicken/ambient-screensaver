import unittest

from scripts.idle_launcher import IdleGate


class IdleGateTests(unittest.TestCase):
    def test_launches_once_per_idle_period(self):
        gate = IdleGate(5_000)
        self.assertFalse(gate.should_launch(4_999, False))
        self.assertTrue(gate.should_launch(5_000, False))
        self.assertFalse(gate.should_launch(20_000, False))
        self.assertFalse(gate.should_launch(0, False))
        self.assertTrue(gate.should_launch(5_000, False))

    def test_waits_for_unlock_without_consuming_idle_launch(self):
        gate = IdleGate(5_000)
        self.assertFalse(gate.should_launch(9_000, True))
        self.assertTrue(gate.should_launch(9_000, False))


if __name__ == "__main__":
    unittest.main()
