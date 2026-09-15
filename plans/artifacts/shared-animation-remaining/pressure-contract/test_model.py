import unittest
from model import State, explore


def dispatch(state, token=0):
    return state.move(token).move(token)


class AdmissionContract(unittest.TestCase):
    def test_transfer_and_ack_do_not_refund_live_capacity(self):
        state = dispatch(State(4, 2).admit(2)).move(0).move(0)
        state = state.admit(2)
        self.assertEqual(state.used, 4)
        self.assertEqual(state.acknowledge().used, 4)
        failed = state.admit(1)
        self.assertEqual(failed.reason, ("overflow", 1))
        self.assertEqual(failed.live, state.live)

    def test_replacement_requires_peak_not_net_reservation(self):
        old = dispatch(State(3, 2).admit(2))
        failed = old.grow(0, 2)  # Final replacement fits; old + new does not.
        self.assertEqual(failed.reason, ("overflow", 2))
        self.assertEqual(failed.used, 2)
        enough = dispatch(State(4, 2).admit(2)).grow(0, 2)
        self.assertEqual(enough.used, 4)
        self.assertEqual(enough.free_work_storage(0, 2).used, 2)

    def test_count_cap_does_not_replace_capacity_cap(self):
        self.assertEqual(State(3, 100).admit(4).reason, ("overflow", 4))
        self.assertEqual(State(100, 1).admit(1).admit(1).reason, ("overflow", 1))

    def test_control_progress_at_full_budget_without_forging_receipts(self):
        state = dispatch(State(2, 2).admit(1)).move(0)
        state = state.admit(1).move(1)
        self.assertEqual(state.move(1), state)
        self.assertEqual(state.acknowledge(False), state)
        self.assertEqual(state.acknowledge().move(1).live[-1].owner, "dispatch")
        self.assertEqual(state.fault("stopped").used, 2)

    def test_terminal_reason_persists_until_cleanup_and_cannot_resume(self):
        failed = State(2, 2).admit(2).admit(1)
        for _ in range(1000):
            self.assertEqual(failed.admit(1), failed)
            self.assertEqual(failed.acknowledge(), failed)
            self.assertEqual(failed.fault("stopped"), failed)
        retired = failed.retire(0)
        self.assertEqual(retired.used, 0)
        self.assertEqual(retired.reason, failed.reason)
        self.assertEqual(retired.admit(1), retired)

    def test_fifo_bypass_and_healthy_eviction_are_rejected(self):
        state = State(3, 3).admit(1).admit(1)
        with self.assertRaises(ValueError):
            state.move(1)
        with self.assertRaises(ValueError):
            state.retire(0)

    def test_invalid_limits_and_charges_are_not_unlimited_sentinels(self):
        for amount in (0, -1):
            with self.assertRaises(ValueError):
                State(amount, 2)
            with self.assertRaises(ValueError):
                State(2, amount)
            with self.assertRaises(ValueError):
                State(2, 2).admit(amount)

    def test_unbounded_stall_cannot_be_finite_lossless_and_always_accepting(self):
        state = State(3, 3)
        for _ in range(3):
            state = state.admit(1)
        self.assertEqual(state.admit(1).reason, ("overflow", 1))
        # Ignoring admission to stay 'available' would own four units, not three.
        self.assertGreater(state.used + 1, state.byte_limit)

    def test_bounded_reachable_states(self):
        report = explore(3, 2, max_tokens=2, depth=10)
        self.assertGreater(report["states"], 100)
        self.assertGreater(report["transitions"], report["states"])


if __name__ == "__main__":
    unittest.main()
