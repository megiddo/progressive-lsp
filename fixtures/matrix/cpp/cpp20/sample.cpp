// C++20: concepts, consteval.
template<class T>
concept Integral = true;
consteval int one() { return 1; }
int add(int a, int b) { return a + b; }
