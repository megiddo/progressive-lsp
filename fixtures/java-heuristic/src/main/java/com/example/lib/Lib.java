package com.example.lib;

public class Lib {
    public int id;

    public static String greet(String name) {
        return "hello " + name;
    }

    public static String staticGreet(String name) {
        return greet(name);
    }
}
