package com.example.mill;

/** A mill that grinds. */
public class Mill {
    public static final String HOME_ENV = "MILL_HOME";

    private int stones;

    public Mill(int stones) {
        this.stones = stones;
    }

    public void grind(String grain) {
    }

    public enum Speed {
        SLOW,
        FAST
    }
}
