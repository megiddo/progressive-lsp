// C++23: if consteval / size suffix comment; keep parseable.
constexpr unsigned n = 1u;
int add(int a, int b) { return a + b + int(n); }
