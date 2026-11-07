from .util import checksum


class Store:  # @def Store
    def __init__(self):  # @def Store::__init__
        self.items = []

    def save(self, item):  # @def Store::save
        total = checksum(item)
        self.items.append(f"{item}:{total}")
        self._flush()

    def size(self):  # @def Store::size
        return len(self.items)

    def _flush(self):  # @def Store::_flush
        print("flush", len(self.items))
