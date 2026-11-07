class Cache:  # @def Cache
    def __init__(self, cap: int):  # @def Cache::__init__
        self.cap = cap
        self.data = {}

    def save(self, key):  # @def Cache::save
        if len(self.data) >= self.cap:
            self.evict()
        self.data.get(key)

    def size(self):  # @def Cache::size
        return len(self.data)

    def evict(self):  # @def Cache::evict
        fresh = Cache(self.cap)
        return fresh.size()

    @staticmethod
    def build(cap):  # @def Cache::build
        return Cache(cap)
