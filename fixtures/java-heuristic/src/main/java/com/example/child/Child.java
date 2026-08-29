package com.example.child;

import com.example.base.Base;

public class Child extends Base {
    public String extra() {
        return baseOnly();
    }
}
