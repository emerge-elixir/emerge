"""Abstract admission-credit model, NOT a native allocator or event implementation.

Units are declared capacity charges. Integration must prove reservation precedes
allocation and release follows last-owner disposal; this model cannot prove that.
Matching receipts are an explicit assumption, not minted authority in this model.
"""
from dataclasses import dataclass, replace

OWNERS = ("producer", "buffer", "dispatch", "outbox", "channel", "receiver")


@dataclass(frozen=True)
class Allocation:
    token: int
    units: int
    owner: str = "producer"


@dataclass(frozen=True)
class State:
    byte_limit: int
    record_limit: int
    live: tuple[Allocation, ...] = ()
    next_token: int = 0
    reason: tuple[str, int] | None = None
    awaiting: bool = False
    reserved: int = 0
    released: int = 0

    def __post_init__(self):
        if self.byte_limit <= 0 or self.record_limit <= 0:
            raise ValueError("limits must be positive")

    @property
    def used(self):
        return sum(item.units for item in self.live)

    def fault(self, code, requested=0):
        # First terminal reason wins; no error-history queue and no recovery.
        return self if self.reason else replace(self, reason=(code, requested))

    def admit(self, units):
        if units <= 0:
            raise ValueError("charges must be positive")
        if self.reason:
            return self
        if units > self.byte_limit - self.used or len(self.live) == self.record_limit:
            return self.fault("overflow", units)
        return replace(
            self,
            live=self.live + (Allocation(self.next_token, units),),
            next_token=self.next_token + 1,
            reserved=self.reserved + units,
        )

    def move(self, token):
        if self.reason:
            return self
        item = next(item for item in self.live if item.token == token)
        if item.owner == "receiver":
            raise ValueError("receiver already owns allocation")
        if item != next(other for other in self.live if other.owner == item.owner):
            raise ValueError("FIFO bypass")
        if item.owner == "buffer" and (
            self.awaiting or any(other.owner == "dispatch" for other in self.live)
        ):
            return self
        destination = OWNERS[OWNERS.index(item.owner) + 1]
        return replace(
            self,
            live=tuple(
                replace(other, owner=destination) if other.token == token else other
                for other in self.live
            ),
            awaiting=self.awaiting or destination == "outbox",
        )

    def grow(self, token, units):
        """Reserve coexistence cost BEFORE constructing an effect/replacement."""
        if units <= 0:
            raise ValueError("charges must be positive")
        if self.reason:
            return self
        item = next(item for item in self.live if item.token == token)
        if item.owner != "dispatch":
            raise ValueError("growth belongs to dispatch construction")
        if units > self.byte_limit - self.used:
            return self.fault("overflow", units)
        return replace(
            self,
            live=tuple(
                replace(other, units=other.units + units) if other.token == token else other
                for other in self.live
            ),
            reserved=self.reserved + units,
        )

    def free_work_storage(self, token, units):
        """Assumes old construction storage actually freed, not merely dequeued."""
        item = next(item for item in self.live if item.token == token)
        if item.owner != "dispatch" or not 0 < units < item.units:
            raise ValueError("invalid construction storage disposal")
        return replace(
            self,
            live=tuple(
                replace(other, units=other.units - units) if other.token == token else other
                for other in self.live
            ),
            released=self.released + units,
        )

    def retire(self, token):
        item = next(item for item in self.live if item.token == token)
        if not self.reason and item.owner != "receiver":
            raise ValueError("healthy buffered work cannot be silently evicted")
        return replace(
            self,
            live=tuple(other for other in self.live if other.token != token),
            released=self.released + item.units,
        )

    def acknowledge(self, matching=True):
        if self.reason or not matching:
            return self
        return replace(self, awaiting=False)

    def check(self):
        assert 0 <= self.used <= self.byte_limit
        assert len(self.live) <= self.record_limit
        assert self.reserved - self.released == self.used
        assert len({item.token for item in self.live}) == len(self.live)
        assert all(item.units > 0 and item.owner in OWNERS for item in self.live)


def successors(state, max_tokens):
    yield "stop", state.fault("stopped")
    yield "disconnect", state.fault("peer_lost")
    yield "matching_ack", state.acknowledge()
    yield "foreign_ack", state.acknowledge(False)
    if state.next_token < max_tokens:
        for units in range(1, state.byte_limit + 2):
            yield f"admit:{units}", state.admit(units)
    for item in state.live:
        if state.reason or item.owner == "receiver":
            yield f"dispose:{item.token}", state.retire(item.token)
        if state.reason:
            continue
        if item.owner != "receiver" and item == next(
            other for other in state.live if other.owner == item.owner
        ):
            yield f"move:{item.token}", state.move(item.token)
        if item.owner == "dispatch":
            for units in range(1, state.byte_limit + 2):
                yield f"grow:{item.token}:{units}", state.grow(item.token, units)
            for units in range(1, item.units):
                yield f"free_work:{item.token}:{units}", state.free_work_storage(item.token, units)


def explore(byte_limit, record_limit, max_tokens, depth):
    """Bounded reachable-state enumeration, not proof for arbitrary histories."""
    start = State(byte_limit, record_limit)
    seen = {start}
    frontier = {start}
    transitions = 0
    for _ in range(depth):
        following = set()
        for state in frontier:
            for label, candidate in successors(state, max_tokens):
                transitions += 1
                candidate.check()
                if state.reason:
                    assert candidate.reason == state.reason, label
                if label.startswith("move:") or label.endswith("ack"):
                    assert candidate.used == state.used, label
                if candidate not in seen:
                    following.add(candidate)
        seen.update(following)
        frontier = following
        if not frontier:
            break
    return {
        "capacity_units": byte_limit,
        "records": record_limit,
        "max_admitted_tokens": max_tokens,
        "max_depth": depth,
        "states": len(seen),
        "transitions": transitions,
        "remaining_frontier": len(frontier),
        "scope": "abstract declared-charge safety, not native bytes, liveness or clocks",
    }
