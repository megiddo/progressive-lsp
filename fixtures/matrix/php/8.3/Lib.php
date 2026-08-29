<?php
// PHP 8.3: typed class constants
class Lib {
    public const string NAME = "lib";
    public function add(int $a, int $b): int { return $a + $b; }
}
