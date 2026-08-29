# Python 3.13: type defaults
type Alias[T = int] = list[T]
def add(a: int, b: int) -> int:
    return a + b
