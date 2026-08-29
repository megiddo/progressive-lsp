// C++26 window pin: concepts + consteval (representative).
template<class T>
concept Incrementable = requires(T t) { ++t; };
consteval int one() { return 1; }
int add(int a, int b) { return a + b + one(); }
