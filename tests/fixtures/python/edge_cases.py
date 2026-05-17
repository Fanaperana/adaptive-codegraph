# PYTHON EDGE CASES — comprehensive test for tricky patterns

from __future__ import annotations
import abc
import functools
import contextlib
from typing import (
    TypeVar, Generic, Protocol, ClassVar, Final,
    Optional, Union, Callable, Iterator, Generator,
    TypedDict, NamedTuple, overload, runtime_checkable,
)
from dataclasses import dataclass, field
from enum import Enum, Flag, auto

# === 1. Complex enum types ===
class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

class Permission(Flag):
    READ = auto()
    WRITE = auto()
    EXECUTE = auto()
    ALL = READ | WRITE | EXECUTE

# === 2. TypedDict and NamedTuple ===
class UserDict(TypedDict, total=False):
    name: str
    age: int
    email: Optional[str]

class Point(NamedTuple):
    x: float
    y: float
    z: float = 0.0

# === 3. Generic class with bounds ===
T = TypeVar("T")
K = TypeVar("K", bound=str)
V = TypeVar("V")

class Registry(Generic[K, V]):
    _items: dict[K, V]

    def __init__(self) -> None:
        self._items = {}

    def register(self, key: K, value: V) -> None:
        self._items[key] = value

    def get(self, key: K) -> Optional[V]:
        return self._items.get(key)

    def __contains__(self, key: K) -> bool:
        return key in self._items

    def __len__(self) -> int:
        return len(self._items)

    def __iter__(self) -> Iterator[K]:
        return iter(self._items)

# === 4. Protocol (structural typing) ===
@runtime_checkable
class Comparable(Protocol):
    def __lt__(self, other: Comparable) -> bool: ...
    def __eq__(self, other: object) -> bool: ...

class Hashable(Protocol):
    def __hash__(self) -> int: ...

# === 5. Abstract base class ===
class Serializer(abc.ABC):
    @abc.abstractmethod
    def serialize(self, data: object) -> bytes: ...

    @abc.abstractmethod
    def deserialize(self, raw: bytes) -> object: ...

    @classmethod
    def create(cls, fmt: str) -> Serializer:
        if fmt == "json":
            return JsonSerializer()
        raise ValueError(f"Unknown format: {fmt}")

class JsonSerializer(Serializer):
    def serialize(self, data: object) -> bytes:
        import json
        return json.dumps(data).encode()

    def deserialize(self, raw: bytes) -> object:
        import json
        return json.loads(raw)

# === 6. Metaclass ===
class SingletonMeta(type):
    _instances: ClassVar[dict] = {}

    def __call__(cls, *args, **kwargs):
        if cls not in cls._instances:
            cls._instances[cls] = super().__call__(*args, **kwargs)
        return cls._instances[cls]

class Database(metaclass=SingletonMeta):
    def __init__(self, url: str = "sqlite://"):
        self.url = url
        self.connected = False

    def connect(self) -> None:
        self.connected = True

    def disconnect(self) -> None:
        self.connected = False

# === 7. Multiple decorators stacked ===
def validate_input(func):
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        for a in args:
            if a is None:
                raise ValueError("None not allowed")
        return func(*args, **kwargs)
    return wrapper

def cache_result(maxsize: int = 128):
    def decorator(func):
        return functools.lru_cache(maxsize=maxsize)(func)
    return decorator

@validate_input
@cache_result(maxsize=64)
def expensive_computation(x: int, y: int) -> int:
    return x ** y

# === 8. Context manager (sync and async) ===
@contextlib.contextmanager
def temp_directory(prefix: str = "tmp"):
    import tempfile, shutil
    path = tempfile.mkdtemp(prefix=prefix)
    try:
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)

@contextlib.asynccontextmanager
async def managed_connection(url: str):
    conn = await _connect(url)
    try:
        yield conn
    finally:
        await conn.close()

async def _connect(url: str):
    return type("Connection", (), {"close": lambda self: None, "url": url})()

# === 9. Property decorators ===
class Temperature:
    def __init__(self, celsius: float = 0.0):
        self._celsius = celsius

    @property
    def celsius(self) -> float:
        return self._celsius

    @celsius.setter
    def celsius(self, value: float) -> None:
        if value < -273.15:
            raise ValueError("Below absolute zero")
        self._celsius = value

    @property
    def fahrenheit(self) -> float:
        return self._celsius * 9 / 5 + 32

    @fahrenheit.setter
    def fahrenheit(self, value: float) -> None:
        self.celsius = (value - 32) * 5 / 9

# === 10. Nested class and function ===
class Outer:
    class Inner:
        class DeepInner:
            VALUE: Final = 42

            @staticmethod
            def compute() -> int:
                return Outer.Inner.DeepInner.VALUE * 2

    @classmethod
    def create_inner(cls) -> Inner:
        return cls.Inner()

# === 11. __slots__ and __init_subclass__ ===
class Optimized:
    __slots__ = ("x", "y", "z")

    def __init__(self, x: int, y: int, z: int):
        self.x = x
        self.y = y
        self.z = z

    def __repr__(self) -> str:
        return f"Optimized({self.x}, {self.y}, {self.z})"

class PluginBase:
    _plugins: ClassVar[list[type]] = []

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        PluginBase._plugins.append(cls)

class LogPlugin(PluginBase):
    pass

class MetricsPlugin(PluginBase):
    pass

# === 12. Overloaded function (typing) ===
@overload
def process(data: str) -> list[str]: ...
@overload
def process(data: bytes) -> list[bytes]: ...
@overload
def process(data: int) -> list[int]: ...

def process(data):
    if isinstance(data, str):
        return data.split()
    elif isinstance(data, bytes):
        return [data[i:i+1] for i in range(len(data))]
    elif isinstance(data, int):
        return list(range(data))
    raise TypeError(f"Unsupported type: {type(data)}")

# === 13. Walrus operator in comprehension ===
RAW_DATA = [1, -2, 3, -4, 5, 0, -6, 7]

positive_squares = [
    square
    for x in RAW_DATA
    if (square := x * x) > 0 and x > 0
]

# === 14. Complex constants ===
MAX_RETRIES: Final[int] = 5
DEFAULT_HEADERS: Final[dict[str, str]] = {
    "Content-Type": "application/json",
    "Accept": "application/json",
}
SENTINEL = object()

# === 15. Async patterns ===
async def gather_with_limit(coros, limit: int = 10):
    import asyncio
    semaphore = asyncio.Semaphore(limit)

    async def bounded(coro):
        async with semaphore:
            return await coro

    return await asyncio.gather(*(bounded(c) for c in coros))

# === 16. Generator with send() ===
def accumulator() -> Generator[float, float, float]:
    total = 0.0
    while True:
        value = yield total
        if value is None:
            break
        total += value
    return total
