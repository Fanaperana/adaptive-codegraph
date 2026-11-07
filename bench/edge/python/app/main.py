from .models import Store, Cache, Child
from .util import checksum as cs, format_name
from . import util


class Service:  # @def Service
    def __init__(self):  # @def Service::__init__
        self.store = Store()
        self.cache = Cache()

    def persist(self, x):  # @def Service::persist
        self.store.save(x)


def same_name_other_type():  # @def same_name_other_type
    c = Cache()
    c.save(1)


def alias_call():  # @def alias_call
    return cs("x")


def module_call():  # @def module_call
    return util.checksum("y")


def mentions_only():  # @def mentions_only
    # checksum is computed elsewhere
    label = "call checksum() later"
    return label


def shadowed():  # @def shadowed
    format_name = lambda s: s
    return format_name("z")


def callback_use(items):  # @def callback_use
    return list(map(format_name, items))


def via_subclass():  # @def via_subclass
    return Child().load()
