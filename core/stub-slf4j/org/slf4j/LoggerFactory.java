package org.slf4j;
public class LoggerFactory {
  public static Logger getLogger(String n){
    return new Logger(){
      public void error(String s,Object a,Object b){ System.out.println("[ERROR] "+s+" "+a); }
      public void info(String s,Object... a){ System.out.println("[INFO] "+s); }
    };
  }
}
