from app.base import Repository
from app.db import connect, Database


class UserRepo(Repository):  # @def UserRepo
    def validate(self, obj):  # @def UserRepo::validate
        super().validate(obj)
        if not obj.get("name"):
            raise ValueError("name")

    def by_name(self, name):  # @def UserRepo::by_name
        return self.find(name)


class Service:  # @def Service
    def __init__(self, db: Database):  # @def Service::__init__
        self.repo = UserRepo(db)
        self.db = db

    def register(self, name):  # @def Service::register
        user = {"name": name}
        self.repo.save(user)
        return self.repo.by_name(name)


def make_service():  # @def make_service
    db = connect()
    svc = Service(db)
    svc.register("ann")
    return svc
