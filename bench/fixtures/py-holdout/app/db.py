class Database:  # @def Database
    def __init__(self):  # @def Database::__init__
        self.rows = {}

    def get(self, key):  # @def Database::get
        return self.rows.get(key)

    def put(self, obj):  # @def Database::put
        self.rows[id(obj)] = obj


def connect() -> Database:  # @def connect
    return Database()
