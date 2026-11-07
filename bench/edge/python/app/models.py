class Store:  # @def Store
    def save(self, x):  # @def Store::save
        return x


class Cache:  # @def Cache
    def save(self, x):  # @def Cache::save
        return x


class Base:  # @def Base
    def load(self):  # @def Base::load
        return 1


class Child(Base):  # @def Child
    def load(self):  # @def Child::load
        return super().load() + 1
