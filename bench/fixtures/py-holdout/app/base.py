class Repository:  # @def Repository
    def __init__(self, db):  # @def Repository::__init__
        self.db = db

    def find(self, key):  # @def Repository::find
        return self.db.get(key)

    def save(self, obj):  # @def Repository::save
        self.validate(obj)
        self.db.put(obj)

    def validate(self, obj):  # @def Repository::validate
        if obj is None:
            raise ValueError("empty")
