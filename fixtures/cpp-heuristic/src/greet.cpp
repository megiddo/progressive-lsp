#include "../include/greet.hpp"

namespace demo {
int Greeter::greet(int n) { return n + 1; }

class Named {
public:
  int greet(int n) { return n; }
};
}
