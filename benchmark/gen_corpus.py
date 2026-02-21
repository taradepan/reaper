#!/usr/bin/env python3
"""
Diverse benchmark corpus generator — reaper vs ruff comparison.

Generates four tiers of Python test files that cover the full breadth of
real-world Python patterns:

  small/       — 40 files, ~90 lines each   (focused modules)
  medium/      — 20 files, ~200 lines each  (realistic service modules)
  large/       — 8 files,  ~600 lines each  (large real-world modules)
  edge_cases/  — 30 hand-crafted files, one tricky pattern each

Pattern coverage (vs previous corpus):
  NEW: async/await, @dataclass, Enum, Protocol, TypeVar/Generic, NamedTuple,
       @property, @staticmethod/@classmethod, __slots__, ABC + abstractmethod,
       try/except import fallbacks, if __name__ == "__main__", global/nonlocal,
       lambda, comprehensions (list/dict/set/generator), walrus in all contexts,
       match/case, functools (wraps/lru_cache/partial), contextmanager,
       multiple inheritance, __init_subclass__, __class_getitem__, f-strings,
       star-unpacking, exception chaining, typing extensions (Literal, Final,
       TypedDict, ParamSpec, Concatenate), overloaded functions, slots classes,
       cached_property, ChainMap, deque, heapq, bisect, conditional imports.
"""

import os
import textwrap

ROOT = os.path.dirname(__file__)
SMALL_DIR  = os.path.join(ROOT, "corpus", "small")
MEDIUM_DIR = os.path.join(ROOT, "corpus", "medium")
LARGE_DIR  = os.path.join(ROOT, "corpus", "large")
EDGE_DIR   = os.path.join(ROOT, "corpus", "edge_cases")


# ─────────────────────────────────────────────────────────────────────────────
# Small modules — one dominant pattern per file
# ─────────────────────────────────────────────────────────────────────────────

def _small_async(i: int) -> str:
    return f'''\
"""Module {i}: async/await patterns."""
from __future__ import annotations
import asyncio
import aiohttp          # RP001: unused
import contextlib
from typing import AsyncGenerator, Optional

_SESSION_TIMEOUT = 30   # used below


async def fetch_data(url: str, timeout: int = _SESSION_TIMEOUT) -> bytes:
    """Used — called in main."""
    async with contextlib.AsyncExitStack() as stack:
        await asyncio.sleep(0)          # just a placeholder
        _ = stack                       # satisfy linter
    return b""


async def unused_fetcher(endpoint: str, retries: int) -> Optional[str]:
    """Never called — RP003."""
    dead_local = endpoint.strip()       # RP002: dead_local
    for _attempt in range(retries):
        await asyncio.sleep(0.1)
    return None


async def async_generator_demo(limit: int) -> AsyncGenerator[int, None]:
    """Used in module footer."""
    for n in range(limit):
        if n % 2 == 0:
            yield n
        else:
            yield n * 2
    dead_after_loop = True              # RP002


async def _private_helper(x: int) -> int:
    """Private — exempt from RP003."""
    return x + 1


async def main_{i}() -> None:
    data = await fetch_data("http://example.com")
    async for val in async_generator_demo(10):
        print(val, data)


if __name__ == "__main__":
    asyncio.run(main_{i}())
'''


def _small_dataclass(i: int) -> str:
    return f'''\
"""Module {i}: dataclass patterns."""
from __future__ import annotations
import dataclasses
import datetime
from dataclasses import dataclass, field, KW_ONLY
from typing import ClassVar, Optional

_VERSION = "1.0.{i}"   # used in class body


@dataclass
class Config_{i}:
    """Used — instantiated below."""
    host: str
    port: int = 8080
    debug: bool = False
    tags: list[str] = field(default_factory=list)
    _version: ClassVar[str] = _VERSION      # ClassVar, not instance field
    KW_ONLY: ClassVar[None] = None          # just to reference the import


@dataclass(frozen=True)
class Coordinate_{i}:
    """Used — created below."""
    x: float
    y: float
    created_at: datetime.datetime = field(
        default_factory=datetime.datetime.now
    )

    def distance_to(self, other: "Coordinate_{i}") -> float:
        return ((self.x - other.x) ** 2 + (self.y - other.y) ** 2) ** 0.5


@dataclass
class UnusedRecord_{i}:
    """Never used — RP004."""
    name: str
    value: int = 0


def process_config_{i}(cfg: Config_{i}) -> str:
    """Used below."""
    unused_copy = dataclasses.asdict(cfg)   # RP002: unused_copy
    return f"{{cfg.host}}:{{cfg.port}}"


cfg_{i} = Config_{i}(host="localhost")
coord_a = Coordinate_{i}(0.0, 0.0)
coord_b = Coordinate_{i}(3.0, 4.0)
_dist = coord_a.distance_to(coord_b)
_label = process_config_{i}(cfg_{i})
print(_label, _dist)
'''


def _small_enum(i: int) -> str:
    return f'''\
"""Module {i}: enum patterns."""
from __future__ import annotations
import enum
from enum import Enum, IntEnum, Flag, auto
import functools

_PREFIX = "STATUS"   # used in Status body


class Status_{i}(Enum):
    """Used — referenced below."""
    PENDING  = f"{{_PREFIX}}_PENDING"
    RUNNING  = f"{{_PREFIX}}_RUNNING"
    DONE     = f"{{_PREFIX}}_DONE"
    FAILED   = f"{{_PREFIX}}_FAILED"

    def is_terminal(self) -> bool:
        return self in (Status_{i}.DONE, Status_{i}.FAILED)


class Permission_{i}(Flag):
    """Used — combined below."""
    READ    = auto()
    WRITE   = auto()
    EXECUTE = auto()
    ALL     = READ | WRITE | EXECUTE


class Priority_{i}(IntEnum):
    """Never used — RP004."""
    LOW    = 1
    MEDIUM = 2
    HIGH   = 3


@functools.lru_cache(maxsize=None)
def status_label_{i}(s: Status_{i}) -> str:
    """Used below."""
    return s.value.lower()


def unused_enum_helper_{i}(p: Permission_{i}) -> str:
    """Never called — RP003."""
    dead = p.name      # RP002
    return dead


current = Status_{i}.RUNNING
label = status_label_{i}(current)
perms = Permission_{i}.READ | Permission_{i}.WRITE
print(label, current.is_terminal(), perms)
'''


def _small_protocol(i: int) -> str:
    return f'''\
"""Module {i}: Protocol / TypeVar / Generic patterns."""
from __future__ import annotations
from typing import Protocol, TypeVar, Generic, runtime_checkable, Iterator
import abc

T = TypeVar("T")
T_co = TypeVar("T_co", covariant=True)
S = TypeVar("S", bound="Comparable_{i}")   # used in Generic class


@runtime_checkable
class Drawable_{i}(Protocol):
    """Protocol — referenced in isinstance check below."""
    def draw(self) -> str: ...
    def resize(self, factor: float) -> None: ...


class Comparable_{i}(Protocol):
    """Protocol — referenced in Generic below."""
    def __lt__(self: S, other: S) -> bool: ...


class Container_{i}(Generic[T]):
    """Generic class — used below."""
    def __init__(self) -> None:
        self._items: list[T] = []

    def add(self, item: T) -> None:
        self._items.append(item)

    def __iter__(self) -> Iterator[T]:
        return iter(self._items)

    def __len__(self) -> int:
        return len(self._items)


class UnusedRegistry_{i}(Generic[T_co]):
    """Never referenced — RP004."""
    pass


class Circle_{i}:
    """Implements Drawable_{i}."""
    def __init__(self, r: float) -> None:
        self.r = r

    def draw(self) -> str:
        return f"Circle(r={{self.r}})"

    def resize(self, factor: float) -> None:
        self.r *= factor


def render_all_{i}(items: list[Drawable_{i}]) -> list[str]:
    """Used below."""
    return [item.draw() for item in items]


def unused_generic_fn_{i}(c: Container_{i}[int]) -> int:
    """Never called — RP003."""
    dead_len = len(c)    # RP002
    return dead_len


box: Container_{i}[Circle_{i}] = Container_{i}()
box.add(Circle_{i}(1.0))
box.add(Circle_{i}(2.5))
print(render_all_{i}(list(box)))
assert isinstance(Circle_{i}(1.0), Drawable_{i})
'''


def _small_properties(i: int) -> str:
    return f'''\
"""Module {i}: @property, @classmethod, @staticmethod, __slots__."""
from __future__ import annotations
import math
import weakref
from typing import Optional

_DEFAULT_PRECISION = 6   # used in Vector body


class Vector_{i}:
    """Used — instantiated below."""
    __slots__ = ("_x", "_y", "_z")

    def __init__(self, x: float, y: float, z: float = 0.0) -> None:
        self._x = x
        self._y = y
        self._z = z

    @property
    def magnitude(self) -> float:
        return math.sqrt(self._x**2 + self._y**2 + self._z**2)

    @property
    def x(self) -> float:
        return self._x

    @x.setter
    def x(self, value: float) -> None:
        self._x = value

    @staticmethod
    def zero() -> "Vector_{i}":
        return Vector_{i}(0.0, 0.0, 0.0)

    @classmethod
    def from_tuple(cls, t: tuple[float, ...]) -> "Vector_{i}":
        return cls(*t[:3])

    def __repr__(self) -> str:
        prec = _DEFAULT_PRECISION
        return f"Vector({{self._x:.{{prec}}g}}, {{self._y:.{{prec}}g}}, {{self._z:.{{prec}}g}})"

    def dot(self, other: "Vector_{i}") -> float:
        return self._x * other._x + self._y * other._y + self._z * other._z


class UnusedSlottedClass_{i}:
    """Never used — RP004."""
    __slots__ = ("value",)
    def __init__(self, value: int) -> None:
        self.value = value


def unused_vector_op_{i}(v: Vector_{i}, scale: float, extra: str) -> str:
    """Never called — RP003; extra is RP008."""
    dead_mag = v.magnitude   # RP002
    return f"{{dead_mag}}"


v1 = Vector_{i}(1.0, 2.0, 3.0)
v2 = Vector_{i}.from_tuple((4.0, 5.0, 6.0))
v0 = Vector_{i}.zero()
_dot = v1.dot(v2)
_ref = weakref.ref(v1)
print(repr(v1), repr(v2), repr(v0), _dot, _ref)
'''


def _small_comprehensions(i: int) -> str:
    return f'''\
"""Module {i}: comprehensions, walrus, generator expressions."""
from __future__ import annotations
import itertools
import collections
import statistics
from typing import Iterable

_THRESHOLD_{i} = {i * 3 + 10}   # used in filter


def process_stream_{i}(data: Iterable[int]) -> dict[str, float]:
    """Used below — exercises walrus + comprehensions."""
    items = list(data)

    # walrus in list comprehension filter
    evens   = [y for x in items if (y := x * 2) < _THRESHOLD_{i}]

    # dict comprehension
    squared = {{v: v**2 for v in items if v > 0}}

    # set comprehension
    unique_mods = {{x % 7 for x in items}}

    # generator fed directly into statistics
    mean_val = statistics.mean(x for x in items if x != 0) if items else 0.0

    # nested comprehension
    matrix = [[r * c for c in range(1, 4)] for r in range(1, 4)]
    flat   = list(itertools.chain.from_iterable(matrix))

    unused_counter = collections.Counter(items)   # RP002: never read after

    return {{
        "evens_sum":  float(sum(evens)),
        "squared_n":  float(len(squared)),
        "unique_mods": float(len(unique_mods)),
        "mean":       mean_val,
        "flat_sum":   float(sum(flat)),
    }}


def unused_reducer_{i}(seq: list[int], start: int, padding: str) -> int:
    """Never called — RP003; padding is RP008."""
    acc = start
    for v in seq:
        acc += v
    dead = acc * 2   # RP002
    return dead


result_{i} = process_stream_{i}(range(1, {i + 20}))
print(result_{i})
'''


def _small_try_import(i: int) -> str:
    return f'''\
"""Module {i}: try/except import fallbacks and conditional imports."""
from __future__ import annotations
import sys
import os
from typing import TYPE_CHECKING

# Fallback import pattern — both branches bind the same name.
try:
    import ujson as json_lib   # fast path
except ImportError:
    import json as json_lib    # stdlib fallback   # noqa: F401

# Platform-specific import
if sys.platform == "win32":
    import winreg              # RP001 on non-Windows (never used body)
else:
    import grp                 # RP001 on Windows

if TYPE_CHECKING:
    from collections.abc import Mapping  # not flagged — TYPE_CHECKING guard

# Version-guarded feature
if sys.version_info >= (3, 11):
    from tomllib import loads as toml_loads
else:
    try:
        from tomli import loads as toml_loads
    except ImportError:
        def toml_loads(s: str) -> dict:   # type: ignore[misc]
            raise NotImplementedError("install tomli")

_HOME = os.path.expanduser("~")   # used below


def read_config_{i}(path: str) -> dict:
    """Used below."""
    with open(path, "rb") as fh:
        raw = fh.read().decode()
    data = json_lib.loads(raw) if hasattr(json_lib, "loads") else toml_loads(raw)
    return data if isinstance(data, dict) else {{}}


def unused_config_writer_{i}(cfg: dict, dest: str, mode: int) -> None:
    """Never called — RP003; mode is RP008."""
    dead_path = os.path.join(_HOME, dest)   # RP002
    _ = dead_path


cfg_result_{i} = read_config_{i}(os.devnull)
print(cfg_result_{i})
'''


def _small_namedtuple(i: int) -> str:
    return f'''\
"""Module {i}: NamedTuple, TypedDict, Literal, Final."""
from __future__ import annotations
from typing import NamedTuple, TypedDict, Literal, Final, NotRequired
import operator

MAX_RETRIES: Final[int] = {i + 3}
Direction = Literal["north", "south", "east", "west"]


class Point_{i}(NamedTuple):
    """Used — created below."""
    x: float
    y: float
    label: str = "unlabelled"


class ConnOptions_{i}(TypedDict):
    """Used in function signature below."""
    host: str
    port: int
    tls: NotRequired[bool]


class UnusedEvent_{i}(NamedTuple):
    """Never referenced — RP004."""
    kind: str
    payload: bytes


def connect_{i}(opts: ConnOptions_{i}, retries: int = MAX_RETRIES) -> str:
    """Used below."""
    for attempt in range(retries):
        if attempt == retries - 1:
            return f"{{opts['host']}}:{{opts['port']}}"
    return "failed"


def unused_sorter_{i}(pts: list[Point_{i}], reverse: bool, key_fn: object) -> list[Point_{i}]:
    """Never called — RP003; key_fn is RP008."""
    dead_sorted = sorted(pts, key=operator.attrgetter("x"), reverse=reverse)  # RP002
    return dead_sorted


origin_{i} = Point_{i}(0.0, 0.0, "origin")
opts_{i}: ConnOptions_{i} = {{"host": "localhost", "port": {8000 + i}}}
addr_{i} = connect_{i}(opts_{i})
print(origin_{i}, addr_{i})
'''


def _small_abc(i: int) -> str:
    return f'''\
"""Module {i}: ABC, abstractmethod, __init_subclass__, multiple inheritance."""
from __future__ import annotations
import abc
from abc import ABC, abstractmethod
from typing import Any

_REGISTRY_{i}: dict[str, type] = {{}}   # used via __init_subclass__


class PluginBase_{i}(ABC):
    """Used — subclassed below."""

    def __init_subclass__(cls, plugin_name: str = "", **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)
        if plugin_name:
            _REGISTRY_{i}[plugin_name] = cls

    @abstractmethod
    def execute(self, payload: bytes) -> bytes: ...

    @abstractmethod
    def validate(self, payload: bytes) -> bool: ...

    def describe(self) -> str:
        return f"{{type(self).__name__}} plugin"


class Mixin_{i}:
    """Mixin — used via multiple inheritance."""
    def log_action(self, action: str) -> None:
        print(f"[mixin] {{action}}")


class ConcretePlugin_{i}(Mixin_{i}, PluginBase_{i}, plugin_name="concrete_{i}"):
    """Used — instantiated below."""
    def execute(self, payload: bytes) -> bytes:
        self.log_action("execute")
        return payload[::-1]

    def validate(self, payload: bytes) -> bool:
        return len(payload) > 0


class UnusedPlugin_{i}(PluginBase_{i}, plugin_name=""):
    """Never instantiated (not in registry either) — RP004."""
    def execute(self, payload: bytes) -> bytes:
        return payload
    def validate(self, payload: bytes) -> bool:
        return True


def run_plugin_{i}(name: str, data: bytes) -> bytes:
    """Used below."""
    cls = _REGISTRY_{i}.get(name)
    if cls is None:
        return b""
    plugin = cls()
    if not plugin.validate(data):
        dead_msg = "invalid"   # RP002
        return b""
    return plugin.execute(data)


def unused_registry_dump_{i}(prefix: str, indent: int) -> str:
    """Never called — RP003; indent is RP008."""
    lines = [f"{{prefix}}{{k}}" for k in _REGISTRY_{i}]
    dead_result = "\\n".join(lines)   # RP002
    return dead_result


output_{i} = run_plugin_{i}("concrete_{i}", b"hello")
print(output_{i}, _REGISTRY_{i})
'''


def _small_contextmanager(i: int) -> str:
    return f'''\
"""Module {i}: contextmanager, __enter__/__exit__, ExitStack."""
from __future__ import annotations
import contextlib
from contextlib import contextmanager, suppress, ExitStack
import tempfile
import os

_TMP_PREFIX = "reaper_bench_{i}_"   # used in factory


@contextmanager
def temp_workspace_{i}(suffix: str = "") -> contextlib.AbstractContextManager:
    """Used below."""
    with tempfile.TemporaryDirectory(prefix=_TMP_PREFIX, suffix=suffix) as d:
        yield d


class ManagedBuffer_{i}:
    """Context-manager class — used via with-statement below."""
    def __init__(self, size: int) -> None:
        self._size = size
        self._buf: bytearray | None = None

    def __enter__(self) -> bytearray:
        self._buf = bytearray(self._size)
        return self._buf

    def __exit__(self, *_: object) -> bool:
        self._buf = None
        return False


class UnusedTimer_{i}:
    """Never used — RP004."""
    def __enter__(self) -> "UnusedTimer_{i}":
        return self
    def __exit__(self, *_: object) -> bool:
        return False


def process_in_workspace_{i}(data: bytes, verbose: bool) -> int:
    """Used below; verbose is RP008."""
    with temp_workspace_{i}() as d:
        path = os.path.join(d, "data.bin")
        with open(path, "wb") as f:
            f.write(data)
        size = os.path.getsize(path)
    dead_path = path   # RP002 — assigned after with block closes
    return size


def unused_stack_demo_{i}(paths: list[str], mode: str) -> None:
    """Never called — RP003; mode is RP008."""
    with ExitStack() as stack:
        handles = [stack.enter_context(open(p)) for p in paths]
        dead_count = len(handles)   # RP002
    with suppress(OSError):
        pass


with ManagedBuffer_{i}(64) as buf_{i}:
    result_{i} = process_in_workspace_{i}(bytes(buf_{i}), False)
    print(result_{i})
'''


def _small_global_nonlocal(i: int) -> str:
    return f'''\
"""Module {i}: global, nonlocal, closures, mutable-default trap."""
from __future__ import annotations
from typing import Callable

_COUNTER_{i}: int = 0     # global mutated below
_LOG_{i}: list[str] = []  # global mutated below


def increment_{i}(by: int = 1) -> int:
    """Used below."""
    global _COUNTER_{i}
    _COUNTER_{i} += by
    return _COUNTER_{i}


def make_adder_{i}(base: int) -> Callable[[int], int]:
    """Used below — closure over `base`."""
    dead_label = f"adder_{{base}}"   # RP002: created but not returned/used

    def adder(x: int) -> int:
        nonlocal base       # legal but unusual
        result = x + base
        base += 0           # force nonlocal re-bind
        return result

    return adder


def make_logger_{i}(prefix: str) -> Callable[[str], None]:
    """Used below — closure mutates module-level list."""
    def log(msg: str) -> None:
        global _LOG_{i}
        _LOG_{i}.append(f"{{prefix}}: {{msg}}")
    return log


def unused_counter_reset_{i}(confirm: bool, label: str) -> None:
    """Never called — RP003; label is RP008."""
    global _COUNTER_{i}
    if confirm:
        _COUNTER_{i} = 0
    dead_snap = _COUNTER_{i}   # RP002


add5_{i}  = make_adder_{i}(5)
logger_{i} = make_logger_{i}(f"mod{i}")
increment_{i}(3)
logger_{i}("started")
print(add5_{i}(10), _COUNTER_{i}, _LOG_{i})
'''


def _small_match(i: int) -> str:
    return f'''\
"""Module {i}: match/case (structural pattern matching, Python ≥3.10)."""
from __future__ import annotations
from dataclasses import dataclass
from typing import Union

@dataclass
class Point_{i}:
    x: float
    y: float

@dataclass
class Circle_{i}:
    center: Point_{i}
    radius: float

@dataclass
class Rectangle_{i}:
    top_left: Point_{i}
    bottom_right: Point_{i}


Shape_{i} = Union[Point_{i}, Circle_{i}, Rectangle_{i}]


def describe_shape_{i}(shape: Shape_{i}) -> str:
    """Used below."""
    match shape:
        case Point_{i}(x=0.0, y=0.0):
            return "origin"
        case Point_{i}(x=x, y=y):
            return f"point({{x}}, {{y}})"
        case Circle_{i}(radius=r) if r > 100:
            return f"huge circle r={{r}}"
        case Circle_{i}(center=c, radius=r):
            dead_area = 3.14159 * r * r   # RP002 — computed but not returned
            return f"circle at {{c}} r={{r}}"
        case Rectangle_{i}():
            return "rectangle"
        case _:
            return "unknown"


def classify_command_{i}(cmd: object) -> str:
    """Used below."""
    match cmd:
        case {{"action": str(action), "target": str(target)}}:
            return f"{{action}} -> {{target}}"
        case {{"action": str(action)}}:
            return f"{{action}}"
        case [first, *rest]:
            return f"seq: {{first}} + {{len(rest)}} more"
        case None | False | 0:
            return "falsy"
        case _:
            return "other"


def unused_shape_area_{i}(s: Shape_{i}, precision: int) -> float:
    """Never called — RP003; precision is RP008."""
    match s:
        case Circle_{i}(radius=r):
            dead_val = r * r * 3.14159   # RP002
            return dead_val
        case _:
            return 0.0


shapes_{i}: list[Shape_{i}] = [
    Point_{i}(0.0, 0.0),
    Circle_{i}(Point_{i}(1.0, 2.0), 5.0),
    Rectangle_{i}(Point_{i}(0.0, 0.0), Point_{i}(3.0, 4.0)),
]
for _s in shapes_{i}:
    print(describe_shape_{i}(_s))
print(classify_command_{i}({{"action": "run", "target": "tests"}}))
'''


# Map of small-module flavour names → generator functions
SMALL_FLAVOURS = [
    _small_async,
    _small_dataclass,
    _small_enum,
    _small_protocol,
    _small_properties,
    _small_comprehensions,
    _small_try_import,
    _small_namedtuple,
    _small_abc,
    _small_contextmanager,
    _small_global_nonlocal,
    _small_match,
]


# ─────────────────────────────────────────────────────────────────────────────
# Medium modules — realistic service/library modules (~200 lines)
# ─────────────────────────────────────────────────────────────────────────────

def _medium_cache_service(i: int) -> str:
    return f'''\
"""Medium module {i}: cache service with TTL, eviction, and metrics."""
from __future__ import annotations
import time
import threading
import hashlib
import pickle
import weakref
import heapq
from dataclasses import dataclass, field
from typing import Any, Callable, Generic, Iterator, Optional, TypeVar
from functools import wraps

V = TypeVar("V")

_DEFAULT_TTL    = 300
_DEFAULT_MAX    = 1024
_HASH_ALG       = "sha256"     # used in CacheKey


@dataclass(order=True)
class CacheEntry_{i}(Generic[V]):
    expires_at: float
    key: str = field(compare=False)
    value: V  = field(compare=False)   # type: ignore[assignment]
    hits: int = field(default=0, compare=False)


class TTLCache_{i}(Generic[V]):
    """Thread-safe TTL cache — used in module footer."""

    def __init__(self, ttl: float = _DEFAULT_TTL, maxsize: int = _DEFAULT_MAX) -> None:
        self._ttl    = ttl
        self._max    = maxsize
        self._store: dict[str, CacheEntry_{i}[V]] = {{}}
        self._heap:  list[CacheEntry_{i}[V]] = []
        self._lock   = threading.RLock()
        self._hits   = 0
        self._misses = 0

    def _make_key(self, raw: str) -> str:
        h = hashlib.new(_HASH_ALG)
        h.update(raw.encode())
        return h.hexdigest()

    def get(self, key: str) -> Optional[V]:
        k = self._make_key(key)
        with self._lock:
            entry = self._store.get(k)
            if entry is None or entry.expires_at < time.monotonic():
                self._misses += 1
                return None
            entry.hits += 1
            self._hits += 1
            return entry.value

    def set(self, key: str, value: V, ttl: Optional[float] = None) -> None:
        k   = self._make_key(key)
        exp = time.monotonic() + (ttl or self._ttl)
        with self._lock:
            if len(self._store) >= self._max:
                self._evict_one()
            entry = CacheEntry_{i}(expires_at=exp, key=k, value=value)
            self._store[k] = entry
            heapq.heappush(self._heap, entry)

    def _evict_one(self) -> None:
        while self._heap:
            oldest = heapq.heappop(self._heap)
            if oldest.key in self._store:
                del self._store[oldest.key]
                return

    def invalidate(self, key: str) -> bool:
        k = self._make_key(key)
        with self._lock:
            return self._store.pop(k, None) is not None

    @property
    def stats(self) -> dict[str, int]:
        return {{"hits": self._hits, "misses": self._misses, "size": len(self._store)}}

    def __iter__(self) -> Iterator[str]:
        with self._lock:
            return iter(list(self._store.keys()))

    def __len__(self) -> int:
        return len(self._store)


def cached_{i}(ttl: float = _DEFAULT_TTL) -> Callable:
    """Decorator — used below."""
    def decorator(fn: Callable) -> Callable:
        _cache: TTLCache_{i}[Any] = TTLCache_{i}(ttl=ttl)

        @wraps(fn)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            key_src = repr(args) + repr(sorted(kwargs.items()))
            hit = _cache.get(key_src)
            if hit is not None:
                return hit
            result = fn(*args, **kwargs)
            _cache.set(key_src, result)
            return result
        return wrapper
    return decorator


@cached_{i}(ttl=60.0)
def expensive_compute_{i}(n: int, factor: float) -> list[float]:
    """Used in footer."""
    return [n * factor * k for k in range(n)]


def _internal_cleanup_{i}(cache: TTLCache_{i}[Any]) -> int:
    """Private — exempt from RP003."""
    now = time.monotonic()
    removed = 0
    keys_to_del = [e.key for e in cache._heap if e.expires_at < now]
    for k in keys_to_del:
        cache._store.pop(k, None)
        removed += 1
    return removed


def unused_cache_warmer_{i}(
    cache: TTLCache_{i}[Any],
    keys: list[str],
    loader: Callable[[str], Any],
    timeout: float,    # RP008: timeout not used in body
) -> int:
    """Never called — RP003."""
    loaded = 0
    for k in keys:
        val = loader(k)
        cache.set(k, val)
        loaded += 1
    dead_stat = cache.stats   # RP002
    return loaded


def unused_weak_proxy_{i}(obj: Any, label: str) -> None:
    """Never called — RP003; label is RP008."""
    ref = weakref.ref(obj)
    dead_deref = ref()   # RP002
    _ = dead_deref
    if pickle.HIGHEST_PROTOCOL > 0:
        pass


_shared_cache_{i}: TTLCache_{i}[list[float]] = TTLCache_{i}(ttl=120.0, maxsize=256)
_shared_cache_{i}.set("pi", [3.14159, 2.71828])
_val_{i} = expensive_compute_{i}({i + 5}, 1.5)
print(_shared_cache_{i}.stats, len(_val_{i}))
'''


def _medium_pipeline(i: int) -> str:
    return f'''\
"""Medium module {i}: data pipeline with stages, transforms, and error handling."""
from __future__ import annotations
import abc
import csv
import io
import json
import logging
import re
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Any, Callable, ClassVar, Generator, Iterator

log = logging.getLogger(__name__)

_VERSION_{i}   = "0.{i}.0"
_MAX_ERRORS = 100


@dataclass
class Record_{i}:
    data:  dict[str, Any]
    index: int
    source: str = ""
    errors: list[str] = field(default_factory=list)

    def add_error(self, msg: str) -> None:
        self.errors.append(msg)

    @property
    def is_valid(self) -> bool:
        return len(self.errors) == 0


class Stage_{i}(abc.ABC):
    name: ClassVar[str] = "base"

    @abc.abstractmethod
    def process(self, rec: Record_{i}) -> Record_{i}: ...

    def __repr__(self) -> str:
        return f"Stage({{self.name}})"


class ValidatorStage_{i}(Stage_{i}):
    name = "validator"

    def __init__(self, required: list[str], pattern: str = r".*") -> None:
        self._required = required
        self._re = re.compile(pattern)

    def process(self, rec: Record_{i}) -> Record_{i}:
        for field_name in self._required:
            if field_name not in rec.data:
                rec.add_error(f"missing: {{field_name}}")
        for v in rec.data.values():
            if isinstance(v, str) and not self._re.match(v):
                rec.add_error(f"pattern mismatch: {{v!r}}")
        return rec


class TransformStage_{i}(Stage_{i}):
    name = "transform"

    def __init__(self, transforms: dict[str, Callable[[Any], Any]]) -> None:
        self._transforms = transforms

    def process(self, rec: Record_{i}) -> Record_{i}:
        for k, fn in self._transforms.items():
            if k in rec.data:
                try:
                    rec.data[k] = fn(rec.data[k])
                except Exception as exc:
                    rec.add_error(str(exc))
        return rec


class UnusedAggregatorStage_{i}(Stage_{i}):
    """Never instantiated — RP004."""
    name = "aggregator"
    def process(self, rec: Record_{i}) -> Record_{i}:
        return rec


class Pipeline_{i}:
    """Used in footer."""
    def __init__(self, stages: list[Stage_{i}]) -> None:
        self._stages = stages
        self._error_counts: dict[str, int] = defaultdict(int)

    def run(self, records: Iterator[Record_{i}]) -> Generator[Record_{i}, None, None]:
        for rec in records:
            for stage in self._stages:
                rec = stage.process(rec)
                if not rec.is_valid:
                    self._error_counts[stage.name] += 1
                    if sum(self._error_counts.values()) >= _MAX_ERRORS:
                        log.warning("max errors reached")
                        return
            yield rec

    @property
    def error_summary(self) -> dict[str, int]:
        return dict(self._error_counts)


def records_from_csv_{i}(src: str) -> Iterator[Record_{i}]:
    """Used below."""
    reader = csv.DictReader(io.StringIO(src))
    for i, row in enumerate(reader):
        yield Record_{i}(data=dict(row), index=i)


def records_from_json_{i}(src: str) -> Iterator[Record_{i}]:
    """Used below."""
    rows = json.loads(src)
    for i, row in enumerate(rows if isinstance(rows, list) else [rows]):
        yield Record_{i}(data=row, index=i)


def _format_error_report_{i}(counts: dict[str, int]) -> str:
    """Private — exempt from RP003."""
    return "; ".join(f"{{k}}={{v}}" for k, v in counts.items())


def unused_pipeline_builder_{i}(
    required: list[str],
    transforms: dict[str, Callable],
    strict: bool,      # RP008
) -> Pipeline_{i}:
    """Never called — RP003."""
    stages: list[Stage_{i}] = [
        ValidatorStage_{i}(required),
        TransformStage_{i}(transforms),
    ]
    dead_repr = repr(stages)   # RP002
    return Pipeline_{i}(stages)


_CSV_{i} = "name,age\\nAlice,30\\nBob,25"
_pipe_{i} = Pipeline_{i}([
    ValidatorStage_{i}(["name", "age"], r".+"),
    TransformStage_{i}({{"age": int}}),
])
_out_{i} = list(_pipe_{i}.run(records_from_csv_{i}(_CSV_{i})))
print(_pipe_{i}.error_summary, [r.data for r in _out_{i}])
'''


# ─────────────────────────────────────────────────────────────────────────────
# Large modules — heavy real-world modules (~600 lines)
# ─────────────────────────────────────────────────────────────────────────────

def _large_orm(i: int) -> str:
    """Simulates a mini ORM with model, query-builder, migrations."""
    return f'''\
"""Large module {i}: mini ORM — Model, QuerySet, Migration, Connection."""
from __future__ import annotations
import abc
import copy
import hashlib
import itertools
import json
import logging
import re
import sqlite3
import threading
import time
import weakref
from collections import defaultdict
from contextlib import contextmanager
from dataclasses import dataclass, field
from enum import Enum, auto
from functools import wraps, lru_cache
from typing import (
    Any, Callable, ClassVar, Generator, Generic, Iterator,
    Optional, TypeVar, Union, overload,
)

log = logging.getLogger(__name__)
T   = TypeVar("T", bound="Model_{i}")
_THREAD_LOCAL = threading.local()
_SCHEMA_VERSION = {i}


# ── Enums ─────────────────────────────────────────────────────────────────────

class FieldType_{i}(Enum):
    INTEGER  = auto()
    TEXT     = auto()
    REAL     = auto()
    BLOB     = auto()
    BOOLEAN  = auto()
    JSON     = auto()


class Ordering_{i}(Enum):
    ASC  = "ASC"
    DESC = "DESC"


class UnusedIsolation_{i}(Enum):
    """Never referenced — RP004."""
    SERIALIZABLE   = "SERIALIZABLE"
    REPEATABLE_READ = "REPEATABLE_READ"
    READ_COMMITTED = "READ_COMMITTED"


# ── Field descriptors ─────────────────────────────────────────────────────────

@dataclass
class Field_{i}:
    type:     FieldType_{i}
    null:     bool  = False
    default:  Any   = None
    primary:  bool  = False
    unique:   bool  = False
    index:    bool  = False

    def ddl_fragment(self, name: str) -> str:
        parts = [name, self.type.name]
        if self.primary:
            parts.append("PRIMARY KEY")
        if not self.null:
            parts.append("NOT NULL")
        if self.unique:
            parts.append("UNIQUE")
        if self.default is not None:
            parts.append(f"DEFAULT {{self.default!r}}")
        return " ".join(parts)


# ── Connection pool ───────────────────────────────────────────────────────────

class ConnectionPool_{i}:
    """Thread-local SQLite connections — used in Session below."""
    def __init__(self, db_path: str, maxconn: int = 5) -> None:
        self._path    = db_path
        self._max     = maxconn
        self._lock    = threading.Lock()
        self._all:    list[sqlite3.Connection] = []
        self._free:   list[sqlite3.Connection] = []
        self._refs:   weakref.WeakValueDictionary = weakref.WeakValueDictionary()

    def acquire(self) -> sqlite3.Connection:
        with self._lock:
            if self._free:
                return self._free.pop()
            if len(self._all) < self._max:
                conn = sqlite3.connect(self._path, check_same_thread=False)
                conn.row_factory = sqlite3.Row
                self._all.append(conn)
                return conn
        time.sleep(0.01)
        return self.acquire()

    def release(self, conn: sqlite3.Connection) -> None:
        with self._lock:
            self._free.append(conn)

    @contextmanager
    def connection(self) -> Generator[sqlite3.Connection, None, None]:
        conn = self.acquire()
        try:
            yield conn
            conn.commit()
        except Exception:
            conn.rollback()
            raise
        finally:
            self.release(conn)

    def close_all(self) -> None:
        with self._lock:
            for conn in self._all:
                conn.close()
            self._all.clear()
            self._free.clear()


# ── ModelMeta ─────────────────────────────────────────────────────────────────

class ModelMeta_{i}(type):
    """Metaclass that collects Field_{i} descriptors."""
    def __new__(mcs, name: str, bases: tuple, ns: dict, **kw: Any) -> "ModelMeta_{i}":
        fields: dict[str, Field_{i}] = {{}}
        for b in bases:
            fields.update(getattr(b, "_fields_{i}", {{}}))
        for attr, val in list(ns.items()):
            if isinstance(val, Field_{i}):
                fields[attr] = val
                ns.pop(attr)
        ns[f"_fields_{i}"] = fields
        return super().__new__(mcs, name, bases, ns)


# ── Model base ────────────────────────────────────────────────────────────────

class Model_{i}(metaclass=ModelMeta_{i}):
    """Base ORM model."""
    _table_{i}: ClassVar[str] = ""
    _fields_{i}: ClassVar[dict[str, Field_{i}]] = {{}}

    def __init__(self, **kwargs: Any) -> None:
        self._data: dict[str, Any] = {{}}
        for k, v in kwargs.items():
            self._data[k] = v

    def __getattr__(self, name: str) -> Any:
        try:
            return self._data[name]
        except KeyError:
            raise AttributeError(name) from None

    def __setattr__(self, name: str, value: Any) -> None:
        if name.startswith("_"):
            object.__setattr__(self, name, value)
        else:
            self._data[name] = value

    @classmethod
    def table_name(cls) -> str:
        return cls._table_{i} or cls.__name__.lower()

    @classmethod
    def create_table_sql(cls) -> str:
        fragments = [
            f.ddl_fragment(n) for n, f in cls._fields_{i}.items()
        ]
        return f"CREATE TABLE IF NOT EXISTS {{cls.table_name()}} ({{', '.join(fragments)}})"

    def to_dict(self) -> dict[str, Any]:
        return copy.deepcopy(self._data)

    def to_json(self) -> str:
        return json.dumps(self._data)


# ── Concrete models ───────────────────────────────────────────────────────────

class User_{i}(Model_{i}):
    _table_{i} = "users_{i}"
    id       = Field_{i}(FieldType_{i}.INTEGER, primary=True)
    username = Field_{i}(FieldType_{i}.TEXT, null=False, unique=True)
    email    = Field_{i}(FieldType_{i}.TEXT, null=False)
    active   = Field_{i}(FieldType_{i}.BOOLEAN, default=True)


class Post_{i}(Model_{i}):
    _table_{i} = "posts_{i}"
    id        = Field_{i}(FieldType_{i}.INTEGER, primary=True)
    user_id   = Field_{i}(FieldType_{i}.INTEGER, null=False, index=True)
    title     = Field_{i}(FieldType_{i}.TEXT, null=False)
    body      = Field_{i}(FieldType_{i}.TEXT, default="")
    published = Field_{i}(FieldType_{i}.BOOLEAN, default=False)


class UnusedAuditLog_{i}(Model_{i}):
    """Model never queried or created — RP004."""
    _table_{i} = "audit_{i}"
    id      = Field_{i}(FieldType_{i}.INTEGER, primary=True)
    action  = Field_{i}(FieldType_{i}.TEXT)
    ts      = Field_{i}(FieldType_{i}.REAL)


# ── QuerySet ──────────────────────────────────────────────────────────────────

class QuerySet_{i}(Generic[T]):
    """Lazy query builder — used via Session below."""
    def __init__(self, model: type[T], pool: ConnectionPool_{i}) -> None:
        self._model  = model
        self._pool   = pool
        self._where: list[str] = []
        self._params: list[Any] = []
        self._order: Optional[tuple[str, Ordering_{i}]] = None
        self._limit: Optional[int] = None

    def filter(self, **kwargs: Any) -> "QuerySet_{i}[T]":
        qs = copy.copy(self)
        for k, v in kwargs.items():
            qs._where.append(f"{{k}} = ?")
            qs._params.append(v)
        return qs

    def order_by(self, col: str, direction: Ordering_{i} = Ordering_{i}.ASC) -> "QuerySet_{i}[T]":
        qs = copy.copy(self)
        qs._order = (col, direction)
        return qs

    def limit(self, n: int) -> "QuerySet_{i}[T]":
        qs = copy.copy(self)
        qs._limit = n
        return qs

    def _build_sql(self) -> tuple[str, list[Any]]:
        table = self._model.table_name()
        sql   = f"SELECT * FROM {{table}}"
        if self._where:
            sql += " WHERE " + " AND ".join(self._where)
        if self._order:
            col, d = self._order
            sql += f" ORDER BY {{col}} {{d.value}}"
        if self._limit is not None:
            sql += f" LIMIT {{self._limit}}"
        return sql, list(self._params)

    def all(self) -> list[T]:
        sql, params = self._build_sql()
        with self._pool.connection() as conn:
            rows = conn.execute(sql, params).fetchall()
        return [self._model(**dict(r)) for r in rows]

    def count(self) -> int:
        table = self._model.table_name()
        where = (" WHERE " + " AND ".join(self._where)) if self._where else ""
        sql   = f"SELECT COUNT(*) FROM {{table}}{{where}}"
        with self._pool.connection() as conn:
            return conn.execute(sql, self._params).fetchone()[0]

    def __iter__(self) -> Iterator[T]:
        return iter(self.all())


# ── Session ───────────────────────────────────────────────────────────────────

class Session_{i}:
    """Facade — used in footer."""
    def __init__(self, pool: ConnectionPool_{i}) -> None:
        self._pool = pool

    def query(self, model: type[T]) -> QuerySet_{i}[T]:
        return QuerySet_{i}(model, self._pool)

    def create(self, model: type[T], **kwargs: Any) -> T:
        obj   = model(**kwargs)
        cols  = ", ".join(obj._data.keys())
        phs   = ", ".join("?" * len(obj._data))
        sql   = f"INSERT INTO {{model.table_name()}} ({{cols}}) VALUES ({{phs}})"
        with self._pool.connection() as conn:
            conn.execute(sql, list(obj._data.values()))
        return obj

    def init_schema(self, *models: type[Model_{i}]) -> None:
        with self._pool.connection() as conn:
            for m in models:
                conn.execute(m.create_table_sql())

    @property
    def version(self) -> int:
        return _SCHEMA_VERSION


# ── Migration helpers ─────────────────────────────────────────────────────────

@lru_cache(maxsize=64)
def _hash_schema_{i}(ddl: str) -> str:
    return hashlib.sha256(ddl.encode()).hexdigest()[:16]


def unused_migration_runner_{i}(
    session: Session_{i},
    migrations: list[str],
    dry_run: bool,
    verbose: bool,     # RP008
) -> list[str]:
    """Never called — RP003."""
    applied: list[str] = []
    for sql in migrations:
        sig = _hash_schema_{i}(sql)
        if not dry_run:
            dead_result = session._pool.acquire()   # RP002
        applied.append(sig)
    return applied


def unused_query_explain_{i}(
    qs: QuerySet_{i}[Any],
    indent: int,      # RP008
) -> str:
    """Never called — RP003."""
    sql, params = qs._build_sql()
    lines = [f"SQL: {{sql}}", f"params: {{params}}"]
    dead_joined = "\\n".join(lines)   # RP002
    return dead_joined


# ── Pattern: overloaded functions ─────────────────────────────────────────────

@overload
def coerce_{i}(value: str)  -> str: ...
@overload
def coerce_{i}(value: int)  -> int: ...
@overload
def coerce_{i}(value: None) -> None: ...
def coerce_{i}(value: Union[str, int, None]) -> Union[str, int, None]:
    """Used in footer."""
    if value is None:
        return None
    if isinstance(value, int):
        return value
    try:
        return int(value)
    except ValueError:
        return value


# ── Pattern: dead branch + unreachable ───────────────────────────────────────

def validate_schema_{i}(ddl: str) -> bool:
    """Used below."""
    if False:
        dead_check = re.match(r"CREATE", ddl)   # RP006 + RP002
    tokens = ddl.split()
    if not tokens:
        return False
    return tokens[0].upper() == "CREATE"


def check_all_fields_{i}(model: type[Model_{i}]) -> list[str]:
    """Used below."""
    issues = []
    for name, fld in model._fields_{i}.items():
        if fld.type not in FieldType_{i}:
            issues.append(name)
    return issues
    dead_return = issues   # RP005 — unreachable


# ── Module footer ─────────────────────────────────────────────────────────────

_pool_{i}   = ConnectionPool_{i}(":memory:")
_sess_{i}   = Session_{i}(_pool_{i})
_sess_{i}.init_schema(User_{i}, Post_{i})
_coerced_{i} = coerce_{i}(str({i}))
_ddl_ok_{i}  = validate_schema_{i}(User_{i}.create_table_sql())
_issues_{i}  = check_all_fields_{i}(User_{i})
_chained_{i} = list(itertools.chain(
    _sess_{i}.query(User_{i}).filter(active=True).all(),
    _sess_{i}.query(Post_{i}).limit({i + 5}).all(),
))
print(_ddl_ok_{i}, _issues_{i}, len(_chained_{i}), _coerced_{i})
_pool_{i}.close_all()
'''


def _large_http_server(i: int) -> str:
    """Simulates a lightweight HTTP routing/middleware framework."""
    return f'''\
"""Large module {i}: micro HTTP framework — Router, Middleware, Request/Response."""
from __future__ import annotations
import abc
import base64
import functools
import hashlib
import hmac
import http.cookies
import json
import logging
import re
import time
import traceback
import urllib.parse
from collections import defaultdict
from dataclasses import dataclass, field
from enum import IntEnum, auto
from typing import Any, Callable, ClassVar, Iterator, Optional

log = logging.getLogger(__name__)
_SECRET_KEY_{i} = hashlib.sha256(b"bench_{i}").hexdigest()


# ── HTTP primitives ───────────────────────────────────────────────────────────

class Method_{i}(IntEnum):
    GET    = auto()
    POST   = auto()
    PUT    = auto()
    PATCH  = auto()
    DELETE = auto()
    HEAD   = auto()
    OPTIONS = auto()


@dataclass
class Headers_{i}:
    _data: dict[str, list[str]] = field(default_factory=dict)

    def set(self, name: str, value: str) -> None:
        self._data[name.lower()] = [value]

    def add(self, name: str, value: str) -> None:
        self._data.setdefault(name.lower(), []).append(value)

    def get(self, name: str, default: str = "") -> str:
        vals = self._data.get(name.lower(), [])
        return vals[0] if vals else default

    def get_all(self, name: str) -> list[str]:
        return self._data.get(name.lower(), [])

    def items(self) -> Iterator[tuple[str, str]]:
        for k, vals in self._data.items():
            for v in vals:
                yield k, v


@dataclass
class Request_{i}:
    method:  Method_{i}
    path:    str
    headers: Headers_{i} = field(default_factory=Headers_{i})
    body:    bytes = b""
    params:  dict[str, str] = field(default_factory=dict)

    @functools.cached_property
    def query(self) -> dict[str, str]:
        if "?" not in self.path:
            return {{}}
        qs = self.path.split("?", 1)[1]
        return dict(urllib.parse.parse_qsl(qs))

    @functools.cached_property
    def json(self) -> Any:
        return json.loads(self.body) if self.body else None

    @property
    def content_type(self) -> str:
        return self.headers.get("content-type")

    @property
    def is_secure(self) -> bool:
        return self.headers.get("x-forwarded-proto") == "https"


@dataclass
class Response_{i}:
    status:  int = 200
    headers: Headers_{i} = field(default_factory=Headers_{i})
    body:    bytes = b""

    @classmethod
    def json_response(cls, data: Any, status: int = 200) -> "Response_{i}":
        r = cls(status=status)
        r.headers.set("content-type", "application/json")
        r.body = json.dumps(data).encode()
        return r

    @classmethod
    def text(cls, text: str, status: int = 200) -> "Response_{i}":
        r = cls(status=status)
        r.headers.set("content-type", "text/plain")
        r.body = text.encode()
        return r

    @classmethod
    def redirect(cls, location: str, permanent: bool = False) -> "Response_{i}":
        r = cls(status=301 if permanent else 302)
        r.headers.set("location", location)
        return r


Handler_{i} = Callable[[Request_{i}], Response_{i}]


# ── Middleware ────────────────────────────────────────────────────────────────

class Middleware_{i}(abc.ABC):
    @abc.abstractmethod
    def __call__(self, request: Request_{i}, next: Handler_{i}) -> Response_{i}: ...


class LoggingMiddleware_{i}(Middleware_{i}):
    """Used in Router below."""
    def __call__(self, request: Request_{i}, next: Handler_{i}) -> Response_{i}:
        start = time.monotonic()
        response = next(request)
        elapsed = time.monotonic() - start
        log.info("{{request.method.name}} {{request.path}} -> {{response.status}} in {{elapsed:.3f}}s")
        return response


class AuthMiddleware_{i}(Middleware_{i}):
    """Used in Router below."""
    def __init__(self, secret: str = _SECRET_KEY_{i}) -> None:
        self._secret = secret

    def _verify(self, token: str) -> bool:
        try:
            raw  = base64.b64decode(token.encode())
            sig  = hmac.new(self._secret.encode(), raw[:32], hashlib.sha256).digest()
            return hmac.compare_digest(sig, raw[32:])
        except Exception:
            return False

    def __call__(self, request: Request_{i}, next: Handler_{i}) -> Response_{i}:
        auth = request.headers.get("authorization")
        if auth.startswith("Bearer "):
            token = auth[7:]
            if not self._verify(token):
                return Response_{i}.json_response({{"error": "Unauthorized"}}, 401)
        return next(request)


class UnusedRateLimitMiddleware_{i}(Middleware_{i}):
    """Never registered — RP004."""
    def __init__(self, rps: int = 100) -> None:
        self._rps = rps
        self._window: dict[str, list[float]] = defaultdict(list)

    def __call__(self, request: Request_{i}, next: Handler_{i}) -> Response_{i}:
        ip = request.headers.get("x-real-ip", "unknown")
        now = time.monotonic()
        hits = [t for t in self._window[ip] if now - t < 1.0]
        self._window[ip] = hits
        if len(hits) >= self._rps:
            return Response_{i}.json_response({{"error": "Too Many Requests"}}, 429)
        self._window[ip].append(now)
        return next(request)


class CORSMiddleware_{i}(Middleware_{i}):
    """Used in Router below."""
    def __init__(self, origins: list[str]) -> None:
        self._origins = set(origins)

    def __call__(self, request: Request_{i}, next: Handler_{i}) -> Response_{i}:
        origin = request.headers.get("origin")
        response = next(request)
        if origin in self._origins or "*" in self._origins:
            response.headers.set("access-control-allow-origin", origin or "*")
        return response


# ── Router ────────────────────────────────────────────────────────────────────

@dataclass
class Route_{i}:
    method:  Method_{i}
    pattern: re.Pattern
    handler: Handler_{i}
    name:    str = ""
    params:  ClassVar[set[str]] = set()


class Router_{i}:
    """Used in footer."""
    def __init__(self) -> None:
        self._routes: list[Route_{i}] = []
        self._middleware: list[Middleware_{i}] = []
        self._error_handlers: dict[int, Handler_{i}] = {{}}
        self._cookies = http.cookies.SimpleCookie()

    def use(self, mw: Middleware_{i}) -> None:
        self._middleware.append(mw)

    def route(self, method: Method_{i}, path: str, name: str = "") -> Callable:
        pattern = re.compile(
            "^" + re.sub(r"<(\w+)>", r"(?P<\1>[^/]+)", path) + "$"
        )
        def decorator(fn: Handler_{i}) -> Handler_{i}:
            self._routes.append(Route_{i}(method, pattern, fn, name))
            return fn
        return decorator

    def error(self, code: int) -> Callable:
        def decorator(fn: Handler_{i}) -> Handler_{i}:
            self._error_handlers[code] = fn
            return fn
        return decorator

    def dispatch(self, request: Request_{i}) -> Response_{i}:
        clean_path = request.path.split("?")[0]
        for route in self._routes:
            m = route.pattern.match(clean_path)
            if m and route.method == request.method:
                request.params = m.groupdict()
                handler = self._wrap_middleware(route.handler)
                try:
                    return handler(request)
                except Exception:
                    log.error(traceback.format_exc())
                    err_handler = self._error_handlers.get(500)
                    if err_handler:
                        return err_handler(request)
                    return Response_{i}.json_response({{"error": "Internal Server Error"}}, 500)
        err_handler = self._error_handlers.get(404)
        if err_handler:
            return err_handler(request)
        return Response_{i}.json_response({{"error": "Not Found"}}, 404)

    def _wrap_middleware(self, handler: Handler_{i}) -> Handler_{i}:
        for mw in reversed(self._middleware):
            _mw = mw
            _handler = handler
            def wrapped(req: Request_{i}, mw=_mw, h=_handler) -> Response_{i}:
                return mw(req, h)
            handler = wrapped
        return handler


# ── Pattern: dead branch + unreachable ───────────────────────────────────────

def _parse_accept_{i}(header: str) -> list[tuple[str, float]]:
    """Used in view below."""
    result = []
    for part in header.split(","):
        part = part.strip()
        if not part:
            continue
        if ";q=" in part:
            mime, q = part.split(";q=", 1)
            result.append((mime.strip(), float(q)))
        else:
            result.append((part, 1.0))
    if False:
        result.sort(key=lambda x: x[1])   # RP006 — dead sort
    return sorted(result, key=lambda x: -x[1])


def _cookie_header_{i}(name: str, value: str, http_only: bool = True) -> str:
    """Used in view below."""
    c = http.cookies.SimpleCookie()
    c[name] = value
    if http_only:
        c[name]["httponly"] = True
    return c.output(header="").strip()
    dead_fallback = f"{{name}}={{value}}"   # RP005


# ── Wire up routes & footer ───────────────────────────────────────────────────

def unused_static_handler_{i}(
    root_dir: str,
    request: Request_{i},
    cache_ttl: int,    # RP008
) -> Response_{i}:
    """Never called — RP003."""
    dead_path = root_dir + request.path   # RP002
    return Response_{i}.text("not found", 404)


_router_{i} = Router_{i}()
_router_{i}.use(LoggingMiddleware_{i}())
_router_{i}.use(AuthMiddleware_{i}())
_router_{i}.use(CORSMiddleware_{i}(["*"]))


@_router_{i}.route(Method_{i}.GET, "/api/{i}/health")
def _health_{i}(req: Request_{i}) -> Response_{i}:
    accept = _parse_accept_{i}(req.headers.get("accept", "*/*"))
    cookie = _cookie_header_{i}("session", "x")
    return Response_{i}.json_response({{"status": "ok", "accept": accept[0][0], "cookie": cookie}})


@_router_{i}.route(Method_{i}.POST, "/api/{i}/echo")
def _echo_{i}(req: Request_{i}) -> Response_{i}:
    return Response_{i}.json_response({{"echo": req.json}})


_test_req_{i} = Request_{i}(
    method=Method_{i}.GET,
    path="/api/{i}/health",
    headers=Headers_{i}(),
)
_resp_{i} = _router_{i}.dispatch(_test_req_{i})
print(_resp_{i}.status, _resp_{i}.body[:60])
'''


# ─────────────────────────────────────────────────────────────────────────────
# Edge-case files — one tricky pattern each
# ─────────────────────────────────────────────────────────────────────────────

EDGE_CASES: dict[str, str] = {

    "ec01_type_checking_guard.py": textwrap.dedent("""\
        \"\"\"TYPE_CHECKING imports must NOT be flagged as RP001.\"\"\"
        from __future__ import annotations
        from typing import TYPE_CHECKING

        if TYPE_CHECKING:
            import json                  # not flagged — dead branch guard
            from pathlib import Path     # not flagged
            from collections.abc import Mapping   # not flagged

        def greet(name: str) -> str:
            return f"hello {name}"

        print(greet("world"))
    """),

    "ec02_annotation_only_no_rp002.py": textwrap.dedent("""\
        \"\"\"Annotation-only declarations must NOT fire RP002.\"\"\"

        def func_with_pure_annotations():
            x: int             # no value — NOT RP002
            y: str             # no value — NOT RP002
            z: list[int]       # no value — NOT RP002
            x = 1              # now x is assigned and used
            return x

        def func_with_typed_assign():
            result: int = 0    # HAS value — if unused, fire RP002
            return result      # used — no fire

        def func_unused_typed():
            dead: int = 42     # HAS value, never read — RP002
            return 0
    """),

    "ec03_augassign_is_use.py": textwrap.dedent("""\
        \"\"\"Augmented assignment counts as both read and write — no RP002.\"\"\"

        def counters():
            total = 0
            total += 10      # augassign — total IS used
            total -= 3
            total *= 2
            return total

        def bitwise_ops():
            flags = 0xFF
            flags &= 0x0F    # used
            flags |= 0x01    # used
            flags ^= 0x10    # used
            return flags

        def string_build():
            msg = ""
            msg += "hello"
            msg += " world"
            return msg
    """),

    "ec04_walrus_contexts.py": textwrap.dedent("""\
        \"\"\"Walrus operator (:=) in various contexts.\"\"\"
        import re

        def walrus_in_while():
            import io
            buf = io.StringIO("hello world foo")
            results = []
            while chunk := buf.read(5):
                results.append(chunk)   # chunk IS used
            return results

        def walrus_in_if():
            data = [1, 2, 3, 4]
            if (n := len(data)) > 2:
                return n               # n IS used
            return 0

        def walrus_in_comprehension():
            nums = range(20)
            return [y for x in nums if (y := x * x) < 100]

        def walrus_unused():
            # walrus target unused after assignment — RP002
            data = [1, 2, 3]
            (n := len(data))    # n assigned but never read
            return 0
    """),

    "ec05_underscore_exempt.py": textwrap.dedent("""\
        \"\"\"Names starting with _ are exempt from RP002/RP009.\"\"\"

        def discard_loop():
            total = 0
            for _ in range(10):    # _ — NOT RP009
                total += 1
            return total

        def discard_unpack():
            pair = (1, 2, 3)
            a, _, c = pair         # _ — NOT RP002
            return a + c

        def discard_private():
            _tmp = expensive_call() if False else 0   # _tmp — NOT RP002
            return 0

        def expensive_call():
            return 42
    """),

    "ec06_locals_vars_suppress.py": textwrap.dedent("""\
        \"\"\"locals()/vars() suppress RP002 for the whole function.\"\"\"

        def uses_locals():
            x = 1
            y = 2
            z = 3
            return locals()    # all vars potentially used — no RP002

        def uses_vars():
            a = "hello"
            b = [1, 2, 3]
            return vars()      # same suppression

        def normal_func():
            dead = 99          # RP002 — no locals() here
            return 0
    """),

    "ec07_dunder_all_protection.py": textwrap.dedent("""\
        \"\"\"__all__ protects imports and functions from RP001/RP003.\"\"\"
        import re
        import sys              # NOT in __all__ and not used → RP001
        import os

        def public_fn():
            \"\"\"In __all__ — no RP003.\"\"\"
            return re.compile(r".")

        def also_public():
            \"\"\"In __all__ — no RP003.\"\"\"
            return os.getcwd()

        def private_fn():
            \"\"\"NOT in __all__ and never called — RP003.\"\"\"
            return 0

        __all__ = ["public_fn", "also_public", "re", "os"]
    """),

    "ec08_star_import_no_flag.py": textwrap.dedent("""\
        \"\"\"Star imports must NOT be flagged.\"\"\"
        from os.path import *     # no RP001 for star
        from typing import *      # no RP001 for star

        result = join("/tmp", "file.txt")   # join came from os.path.*
        x: Optional[int] = None            # Optional from typing.*
    """),

    "ec09_unused_import_aliased.py": textwrap.dedent("""\
        \"\"\"Aliased imports — alias is the local name to check.\"\"\"
        import numpy as np          # RP001: np unused
        import os as operating_sys  # RP001: operating_sys unused
        import sys as system        # USED via 'system'
        from pathlib import Path as P  # USED via 'P'
        from collections import OrderedDict as OD  # RP001: OD unused

        print(system.version)
        p = P("/tmp")
        print(p)
    """),

    "ec10_import_redefined_by_assign.py": textwrap.dedent("""\
        \"\"\"RP007: import clobbered by assignment before any read.\"\"\"
        import os           # RP007 — clobbered before read
        import sys          # NOT RP007 — read before reassignment
        import re           # NOT RP007 — read on RHS of clobber

        print(sys.version)          # sys read first

        re = re.compile(r"\\d+")   # re used on RHS — NOT a clobber
        os = "overwritten"          # os never read before this — RP007

        print(re, os, sys)
    """),

    "ec11_if_false_none_dead_branch.py": textwrap.dedent("""\
        \"\"\"RP006: if False / if None / if 0 are dead branches.\"\"\"

        def check_false():
            if False:            # RP006
                x = never()
            return 1

        def check_none():
            if None:             # RP006
                y = 2
            return 2

        def check_zero():
            if 0:                # RP006
                z = 3
            return 3

        def check_true_not_dead():
            if True:             # NOT dead — always runs
                return 4
            return 5             # RP005: unreachable

        def check_runtime_var_not_dead():
            debug = False
            if debug:            # NOT RP006 — runtime variable
                print("debug")
            return 6
    """),

    "ec12_unreachable_patterns.py": textwrap.dedent("""\
        \"\"\"RP005: code after return/raise/continue/break.\"\"\"

        def after_return():
            return 1
            dead = 2             # RP005

        def after_raise():
            raise ValueError("x")
            also_dead = 3        # RP005

        def after_break():
            for i in range(10):
                break
                unreachable = i  # RP005
            return i

        def after_continue():
            total = 0
            for i in range(10):
                continue
                total += i       # RP005
            return total

        def not_unreachable_conditional():
            for i in range(10):
                if i % 2 == 0:
                    continue
                print(i)         # NOT unreachable — only some iterations skip
    """),

    "ec13_unused_args_patterns.py": textwrap.dedent("""\
        \"\"\"RP008: unused function arguments, with many exemption patterns.\"\"\"
        from abc import ABC, abstractmethod

        def simple(used: int, unused: str) -> int:    # RP008: unused
            return used * 2

        def underscore_ok(_skip: str, used: int) -> int:   # NOT RP008
            return used

        def varargs_ok(*args, **kwargs):               # NOT RP008
            return args, kwargs

        def default_none_still_flagged(x: int, extra: str = "") -> int:  # RP008: extra
            return x

        class Base(ABC):
            @abstractmethod
            def iface(self, arg: int) -> int: ...     # NOT RP008 — abstract

        class Impl(Base):
            def iface(self, arg: int) -> int:         # RP008: arg unused in impl
                return 42

        class WithProperty:
            @property
            def value(self) -> int:
                return 0

            @value.setter
            def value(self, v: int) -> None:          # RP008: v unused
                pass
    """),

    "ec14_unused_loop_vars.py": textwrap.dedent("""\
        \"\"\"RP009: unused loop-control variables.\"\"\"

        def count_only():
            total = 0
            for i in range(10):   # RP009: i not used
                total += 1
            return total

        def use_index():
            for i, v in enumerate([1, 2, 3]):   # i USED
                print(i, v)

        def intentional_discard():
            for _ in range(5):    # _ — NOT RP009
                print("tick")

        def nested_loops():
            matrix = [[1, 2], [3, 4]]
            total = 0
            for row in matrix:          # row USED
                for col in row:         # col USED
                    total += col
            return total

        def enumerated_discard():
            for _, item in enumerate(["a", "b"]):  # _ NOT RP009
                print(item)
    """),

    "ec15_cross_file_anchor.py": textwrap.dedent("""\
        \"\"\"Exported symbols used by ec16_cross_file_user.py.\"\"\"

        def exported_function():
            \"\"\"Called from the user file — no RP003.\"\"\"
            return 42

        def truly_unused():
            \"\"\"Called from nowhere — RP003.\"\"\"
            return 0

        class ExportedClass:
            \"\"\"Instantiated in user file — no RP004.\"\"\"
            pass

        class TrulyUnusedClass:
            \"\"\"Never referenced — RP004.\"\"\"
            pass

        EXPORTED_CONST = "hello"
    """),

    "ec16_cross_file_user.py": textwrap.dedent("""\
        \"\"\"Uses symbols from ec15_cross_file_anchor.py.\"\"\"
        from .ec15_cross_file_anchor import exported_function, ExportedClass

        result = exported_function()
        obj = ExportedClass()
        print(result, obj)
    """),

    "ec17_false_positive_traps.py": textwrap.dedent("""\
        \"\"\"Patterns that must NEVER produce diagnostics.\"\"\"
        import os
        import sys

        # 1. Dynamic access via getattr/globals
        modules = [os, sys]
        for mod in modules:
            print(getattr(mod, "sep", "/"))

        # 2. Slice assignment — not RP002
        def slice_assign():
            arr = [1, 2, 3, 4, 5]
            arr[1:3] = [9, 9]
            return arr

        # 3. List comprehension var reuse
        def comprehension_var():
            return [x * 2 for x in range(10) if x % 2 == 0]

        # 4. Exception variable used
        def exc_used():
            try:
                int("bad")
            except ValueError as e:
                print(e)

        # 5. Walrus in while — chunk is used
        def walrus_while():
            import io
            buf = io.StringIO("abcdefgh")
            while chunk := buf.read(3):
                print(chunk)

        # 6. Nested comprehension
        def nested_comp():
            matrix = [[1, 2], [3, 4]]
            return [cell for row in matrix for cell in row]

        # 7. Assignment in try/except — used in handler
        def try_assign():
            result = None
            try:
                result = int("42")
            except ValueError:
                pass
            return result
    """),

    "ec18_closure_capture.py": textwrap.dedent("""\
        \"\"\"Closure captures of outer variables must not trigger RP002.\"\"\"

        def outer_used_in_closure():
            items = []         # used in inner
            count = 0          # used in inner

            def add(x):
                items.append(x)
                nonlocal count
                count += 1

            add(1)
            add(2)
            return items, count

        def outer_genuinely_unused():
            dead = "never captured"   # RP002 — not used anywhere
            captured = "used"

            def inner():
                return captured

            return inner
    """),

    "ec19_abstract_method_exempt.py": textwrap.dedent("""\
        \"\"\"Abstract methods and protocol stubs must NOT fire RP008.\"\"\"
        from abc import ABC, abstractmethod
        from typing import Protocol

        class AbstractBase(ABC):
            @abstractmethod
            def process(self, data: bytes, flags: int) -> bytes:
                \"\"\"flags not used — but abstract, so no RP008.\"\"\"
                ...

            @abstractmethod
            def validate(self, value: str, strict: bool = False) -> bool: ...

        class ProtocolLike(Protocol):
            def transform(self, item: object, context: dict) -> object: ...

        class ConcreteImpl(AbstractBase):
            def process(self, data: bytes, flags: int) -> bytes:
                # flags genuinely unused in this implementation — RP008
                return data[::-1]

            def validate(self, value: str, strict: bool = False) -> bool:
                return bool(value)  # strict unused — RP008
    """),

    "ec20_conditional_import_fallback.py": textwrap.dedent("""\
        \"\"\"try/except import fallbacks bind the same name — no double-flag.\"\"\"
        try:
            import ujson as json
        except ImportError:
            import json  # type: ignore[no-redef]  # noqa: F401

        try:
            from functools import cache
        except ImportError:
            from functools import lru_cache as cache  # type: ignore[no-redef]

        data = json.dumps({"key": "value"})

        @cache
        def fib(n: int) -> int:
            if n < 2:
                return n
            return fib(n - 1) + fib(n - 2)

        print(data, fib(10))
    """),

    "ec21_property_getter_setter.py": textwrap.dedent("""\
        \"\"\"@property getter/setter/deleter — no false RP008 on self.\"\"\"

        class Temperature:
            def __init__(self, celsius: float = 0.0) -> None:
                self._celsius = celsius

            @property
            def celsius(self) -> float:
                return self._celsius

            @celsius.setter
            def celsius(self, value: float) -> None:
                if value < -273.15:
                    raise ValueError("below absolute zero")
                self._celsius = value

            @celsius.deleter
            def celsius(self) -> None:
                self._celsius = 0.0

            @property
            def fahrenheit(self) -> float:
                return self._celsius * 9 / 5 + 32

            @fahrenheit.setter
            def fahrenheit(self, value: float) -> None:
                self.celsius = (value - 32) * 5 / 9

        t = Temperature(100.0)
        print(t.fahrenheit)
        t.celsius = 0.0
        del t.celsius
    """),

    "ec22_slots_class.py": textwrap.dedent("""\
        \"\"\"__slots__ classes — no RP002 for slot declarations.\"\"\"

        class Point:
            __slots__ = ("x", "y", "z")

            def __init__(self, x: float, y: float, z: float = 0.0) -> None:
                self.x = x
                self.y = y
                self.z = z

            def __repr__(self) -> str:
                return f"Point({self.x}, {self.y}, {self.z})"

            def length(self) -> float:
                return (self.x**2 + self.y**2 + self.z**2) ** 0.5

        class UnusedSlotted:
            \"\"\"Never used — RP004.\"\"\"
            __slots__ = ("value",)
            def __init__(self, v: int) -> None:
                self.value = v

        p = Point(1.0, 2.0, 3.0)
        print(p, p.length())
    """),

    "ec23_lambda_patterns.py": textwrap.dedent("""\
        \"\"\"Lambda expressions — variables holding lambdas should follow same rules.\"\"\"
        from functools import reduce
        import operator

        double   = lambda x: x * 2          # used below
        triple   = lambda x: x * 3          # used below
        dead_fn  = lambda x: x + 99         # RP002 — assigned but never called

        ops = {
            "add": operator.add,
            "mul": operator.mul,
        }

        def apply_all(values: list[int]) -> dict[str, int]:
            doubled  = list(map(double, values))
            tripled  = list(map(triple, values))
            summed   = reduce(operator.add, values, 0)
            return {"doubled_sum": sum(doubled), "tripled_sum": sum(tripled), "total": summed}

        result = apply_all([1, 2, 3, 4, 5])
        print(result, ops)
    """),

    "ec24_overload_pattern.py": textwrap.dedent("""\
        \"\"\"@overload stubs — stub bodies do not count as 'used'.\"\"\"
        from typing import Union, overload

        @overload
        def parse(value: str) -> str: ...
        @overload
        def parse(value: int) -> int: ...
        @overload
        def parse(value: None) -> None: ...

        def parse(value: Union[str, int, None]) -> Union[str, int, None]:
            if value is None:
                return None
            if isinstance(value, int):
                return value
            try:
                return int(value)
            except ValueError:
                return value

        print(parse("42"), parse(99), parse(None))
    """),

    "ec25_exception_chaining.py": textwrap.dedent("""\
        \"\"\"Exception variable scoping — `as e` is cleared after except block.\"\"\"

        def parse_int(s: str) -> int:
            try:
                return int(s)
            except ValueError as e:
                raise RuntimeError(f"bad int: {s!r}") from e

        def multi_except(data: list) -> list:
            results = []
            for item in data:
                try:
                    results.append(int(item))
                except (ValueError, TypeError) as err:
                    print(f"skipping {item!r}: {err}")
            return results

        def bare_except_no_bind():
            try:
                int("bad")
            except ValueError:
                pass            # no 'as e' — nothing to flag

        print(multi_except(["1", "x", "3"]))
    """),

    "ec26_star_unpack.py": textwrap.dedent("""\
        \"\"\"Star unpacking targets — starred name is a real assignment.\"\"\"

        def first_rest():
            first, *rest = [1, 2, 3, 4, 5]
            return first, rest     # both used

        def head_tail_unused_middle():
            head, *_middle, tail = [1, 2, 3, 4, 5]
            return head + tail     # _middle exempt (underscore prefix)

        def all_used():
            a, b, *c = range(10)
            return a + b + sum(c)

        def star_unused():
            x, *dead_rest = [1, 2, 3]   # dead_rest assigned but never read — RP002
            return x

        print(first_rest(), head_tail_unused_middle(), all_used())
    """),

    "ec27_global_nonlocal.py": textwrap.dedent("""\
        \"\"\"global/nonlocal — mutations must not trigger RP002.\"\"\"

        _STATE = 0
        _LOG: list[str] = []

        def increment(n: int = 1) -> int:
            global _STATE
            _STATE += n          # mutation — not RP002
            return _STATE

        def make_counter(start: int):
            count = start

            def bump(by: int = 1) -> int:
                nonlocal count
                count += by      # nonlocal mutation — not RP002
                return count

            return bump

        def read_state() -> int:
            return _STATE        # reads global — not RP002

        counter = make_counter(10)
        increment(5)
        print(counter(1), counter(2), read_state())
    """),

    "ec28_functools_wraps.py": textwrap.dedent("""\
        \"\"\"@functools.wraps decorated functions — decorator must not exempt RP003.\"\"\"
        import functools
        import time
        from typing import Callable, TypeVar

        F = TypeVar("F", bound=Callable)

        def timed(fn: F) -> F:
            \"\"\"Decorator — used below.\"\"\"
            @functools.wraps(fn)
            def wrapper(*args, **kwargs):
                t0 = time.monotonic()
                result = fn(*args, **kwargs)
                print(f"{fn.__name__} took {time.monotonic() - t0:.3f}s")
                return result
            return wrapper  # type: ignore[return-value]

        def retry(times: int = 3) -> Callable[[F], F]:
            \"\"\"Decorator factory — used below.\"\"\"
            def decorator(fn: F) -> F:
                @functools.wraps(fn)
                def wrapper(*args, **kwargs):
                    for attempt in range(times):
                        try:
                            return fn(*args, **kwargs)
                        except Exception:
                            if attempt == times - 1:
                                raise
                return wrapper  # type: ignore[return-value]
            return decorator    # type: ignore[return-value]

        @timed
        @retry(times=2)
        def fetch(url: str) -> str:
            return f"fetched:{url}"

        print(fetch("http://example.com"))
    """),

    "ec29_namedtuple_typed.py": textwrap.dedent("""\
        \"\"\"typing.NamedTuple — fields are declarations, not RP002 targets.\"\"\"
        from typing import NamedTuple, Optional

        class Point(NamedTuple):
            x: float
            y: float
            label: str = "unlabelled"

        class Config(NamedTuple):
            host: str
            port: int = 8080
            tls: bool = False
            timeout: Optional[float] = None

        class UnusedEvent(NamedTuple):
            \"\"\"Never used — RP004.\"\"\"
            kind: str
            payload: bytes

        p = Point(1.0, 2.0)
        cfg = Config("localhost")
        print(p, cfg)
    """),

    "ec30_if_name_main.py": textwrap.dedent("""\
        \"\"\"if __name__ == '__main__' guard — code inside is NOT dead.\"\"\"
        import sys
        import argparse

        def main(argv: list[str]) -> int:
            parser = argparse.ArgumentParser()
            parser.add_argument("--verbose", action="store_true")
            args = parser.parse_args(argv)
            if args.verbose:
                print("verbose mode")
            return 0

        def unused_helper(x: int) -> int:
            \"\"\"Never called — RP003.\"\"\"
            dead = x * 2    # RP002
            return dead

        if __name__ == "__main__":
            sys.exit(main(sys.argv[1:]))
    """),
}


# ─────────────────────────────────────────────────────────────────────────────
# Writer
# ─────────────────────────────────────────────────────────────────────────────

def write_file(path: str, content: str) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    lines = content.count("\n")
    print(f"  wrote {os.path.relpath(path, ROOT)}  ({lines} lines)")


def main() -> None:
    # Trim to a fast, representative subset:
    #   20 small  (~90 lines each)   → covers all 12 flavours at least once
    #    8 medium (~200 lines each)  → 4 cache-service + 4 pipeline
    #    4 large  (~600 lines each)  → 2 ORM + 2 HTTP
    #   15 edge   (hand-crafted)     → first 15 of the 30 defined cases
    SMALL_COUNT  = 20
    MEDIUM_COUNT = 8
    LARGE_COUNT  = 4
    EDGE_NAMES   = list(EDGE_CASES.keys())[:15]

    print(f"=== Generating small corpus ({SMALL_COUNT} files) ===")
    for idx in range(SMALL_COUNT):
        flavour = SMALL_FLAVOURS[idx % len(SMALL_FLAVOURS)]
        write_file(os.path.join(SMALL_DIR, f"small_{idx:02d}.py"), flavour(idx))

    print(f"\n=== Generating medium corpus ({MEDIUM_COUNT} files) ===")
    medium_generators = [_medium_cache_service, _medium_pipeline]
    for idx in range(MEDIUM_COUNT):
        gen = medium_generators[idx % len(medium_generators)]
        write_file(os.path.join(MEDIUM_DIR, f"medium_{idx:02d}.py"), gen(idx))

    print(f"\n=== Generating large corpus ({LARGE_COUNT} files) ===")
    large_generators = [_large_orm, _large_http_server]
    for idx in range(LARGE_COUNT):
        gen = large_generators[idx % len(large_generators)]
        content = gen(idx)
        write_file(os.path.join(LARGE_DIR, f"large_{idx:02d}.py"), content)

    print(f"\n=== Generating edge-case files ({len(EDGE_NAMES)} of {len(EDGE_CASES)}) ===")
    for name in EDGE_NAMES:
        write_file(os.path.join(EDGE_DIR, name), EDGE_CASES[name])

    # Summary
    small_lines  = sum(open(os.path.join(SMALL_DIR,  f)).read().count("\n")
                       for f in os.listdir(SMALL_DIR)  if f.endswith(".py"))
    medium_lines = sum(open(os.path.join(MEDIUM_DIR, f)).read().count("\n")
                       for f in os.listdir(MEDIUM_DIR) if f.endswith(".py"))
    large_lines  = sum(open(os.path.join(LARGE_DIR,  f)).read().count("\n")
                       for f in os.listdir(LARGE_DIR)  if f.endswith(".py"))
    edge_lines   = sum(open(os.path.join(EDGE_DIR,   f)).read().count("\n")
                       for f in os.listdir(EDGE_DIR)   if f.endswith(".py"))

    total_files = SMALL_COUNT + MEDIUM_COUNT + LARGE_COUNT + len(EDGE_NAMES)
    total_lines = small_lines + medium_lines + large_lines + edge_lines
    print(f"""
Done. Summary:
  small/     : {SMALL_COUNT} files, {small_lines:,} lines
  medium/    : {MEDIUM_COUNT} files, {medium_lines:,} lines
  large/     : {LARGE_COUNT} files, {large_lines:,} lines
  edge_cases/: {len(EDGE_NAMES)} files, {edge_lines:,} lines
  ─────────────────────────────────
  total      : {total_files} files, {total_lines:,} lines
""")


if __name__ == "__main__":
    main()