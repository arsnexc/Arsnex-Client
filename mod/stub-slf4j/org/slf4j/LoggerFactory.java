package org.slf4j;

/** Compile-only stub. Messages go to stderr so test output stays readable. */
public final class LoggerFactory {
    public static Logger getLogger(String name) {
        return new Logger() {
            public void error(String s, Object... a) { System.err.println("[ERROR] " + s); }
            public void warn(String s, Object... a)  { System.err.println("[WARN] " + s); }
            public void info(String s, Object... a)  {}
            public void debug(String s, Object... a) {}
        };
    }

    public static Logger getLogger(Class<?> c) { return getLogger(c.getName()); }
}
