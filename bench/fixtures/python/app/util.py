import os


def checksum(text):  # @def util::checksum
    return sum(ord(c) for c in text)


def helper():  # @def util::helper
    path = os.path.join("a", "b")
    return checksum(path)


def join(a, b):  # @def util::join
    return a + b
