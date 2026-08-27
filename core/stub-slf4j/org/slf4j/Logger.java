package org.slf4j;
public interface Logger {
  void error(String s, Object a, Object b);
  void info(String s, Object... a);
}
