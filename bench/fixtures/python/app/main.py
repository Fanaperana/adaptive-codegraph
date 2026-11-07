from .store import Store
from .cache import Cache
from .util import helper, checksum as cs
from . import other


def main():  # @def main
    store = Store()
    store.save("a")
    cache = Cache(4)
    cache.save("b")
    spare = Cache.build(2)
    total = store.size() + cache.size()
    helper()
    other.helper()
    print(total, spare, cs("z"))
    run(store)


def run(store: Store):  # @def run
    store.save("again")

    def inner(x):  # @def run::inner
        return cs(x)

    return inner("q")


def recurse(n):  # @def recurse
    return n if n <= 0 else recurse(n - 1)


if __name__ == "__main__":
    main()
