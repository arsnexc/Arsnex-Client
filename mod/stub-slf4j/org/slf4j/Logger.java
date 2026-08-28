package org.slf4j;

/** Compile-only stub so the module core can be tested without slf4j on the path. */
public interface Logger {
    void error(String s, Object... a);
    void warn(String s, Object... a);
    void info(String s, Object... a);
    void debug(String s, Object... a);
}
