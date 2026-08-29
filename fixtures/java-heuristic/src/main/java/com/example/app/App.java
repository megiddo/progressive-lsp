package com.example.app;

import com.example.lib.Lib;

public class App {
    public String run() {
        String world = "world";
        Lib.staticGreet(world);
        return Lib.greet(world);
    }
}
