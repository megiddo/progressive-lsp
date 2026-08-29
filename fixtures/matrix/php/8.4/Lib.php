<?php
// PHP 8.4: typed constants + hooks-era class
class Lib {
    public const string NAME = "lib";
    public function add(int $a, int $b): int { return $a + $b; }
}
