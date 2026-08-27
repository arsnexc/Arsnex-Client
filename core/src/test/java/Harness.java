import dev.arsex.module.*;
import dev.arsex.module.Module;
import dev.arsex.ui.*;

public class Harness {
  static int pass=0, fail=0;
  static void check(String name, boolean ok, String detail){
    if(ok){pass++; System.out.printf("  PASS  %-46s %s%n", name, detail);}
    else  {fail++; System.out.printf("  FAIL  %-46s %s%n", name, detail);}
  }

  static class Fullbright extends Module {
    boolean applied=false;
    Setting.Slider level;
    Fullbright(){
      super("fullbright","Fullbright","Uniform luminance",Category.VISUAL);
      level = register(new Setting.Slider("Level","Gamma target",15.0,1.0,20.0,0.5,""));
    }
    protected void onEnable(){ applied=true; }
    protected void onDisable(){ applied=false; }
    public String hudSuffix(){ return String.format("%.1f", level.get()); }
  }
  static class Broken extends Module {
    Broken(){ super("broken","Broken","throws",Category.UTILITY); }
    protected void onEnable(){ throw new RuntimeException("boom"); }
  }

  public static void main(String[] a){
    System.out.println("\n\u2500\u2500 ARSEX CORE TEST HARNESS \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\n");

    // ---- MODULE LIFECYCLE ----
    System.out.println("MODULE LIFECYCLE");
    ModuleManager mm = new ModuleManager();
    Fullbright fb = new Fullbright();
    mm.registerAll(fb, new Broken());
    check("registry size", mm.all().size()==2, "2 modules");
    check("starts disabled", !fb.isEnabled() && !fb.applied, "clean initial state");
    fb.toggle();
    check("enable applies effect", fb.isEnabled() && fb.applied, "onEnable ran");
    fb.toggle();
    check("disable reverts effect", !fb.isEnabled() && !fb.applied, "onDisable reverted");

    // fail-safe: a throwing module must not stay enabled
    Module bad = mm.get("broken").orElseThrow();
    bad.setEnabled(true);
    check("throwing module forced off", !bad.isEnabled(), "crash contained");

    // ---- SETTINGS ----
    System.out.println("\nSETTINGS");
    fb.level.set(999.0);
    check("slider clamps to max", fb.level.get()==20.0, "999 -> 20.0");
    fb.level.set(-5.0);
    check("slider clamps to min", fb.level.get()==1.0, "-5 -> 1.0");
    fb.level.set(7.3);
    check("slider quantises to step", fb.level.get()==7.5, "7.3 -> 7.5 (step .5)");
    Setting.Mode mode = new Setting.Mode("Shape","","CROSS","CROSS","DOT","T-BAR");
    mode.cycle(); mode.cycle(); mode.cycle();
    check("mode cycles and wraps", mode.get().equals("CROSS"), "3 cycles -> CROSS");
    mode.set("INVALID");
    check("mode rejects unknown value", mode.get().equals("CROSS"), "INVALID ignored");

    // ---- SEARCH & KEYBINDS ----
    System.out.println("\nSEARCH & KEYBINDS");
    check("search finds by name", mm.search("full").size()==1, "'full' -> 1 hit");
    check("search by category", mm.search("visual").size()==1, "'visual' -> 1 hit");
    check("blank returns all", mm.search("").size()==2, "'' -> all");
    fb.setKeybind(66); bad.setKeybind(66);
    check("conflict detected", mm.conflicts().containsKey(66), "two modules on key 66");
    fb.setKeybind(66); bad.setKeybind(-1);
    mm.rebuildKeybinds();
    check("no false conflict", mm.conflicts().isEmpty(), "after unbinding");
    boolean before = fb.isEnabled();
    mm.onKey(66);
    check("keybind toggles module", fb.isEnabled()!=before, "key 66 fired");

    // ---- SPRING PHYSICS ----
    System.out.println("\nSPRING PHYSICS");
    Spring.reducedMotion=false; Spring.speedScale=1.0;
    Spring s = Spring.snappy(0).target(1);
    int frames=0; double peak=0;
    while(!s.atRest() && frames<2000){ s.update(1.0/144.0); peak=Math.max(peak,s.value()); frames++; }
    double ms = frames*(1000.0/144.0);
    check("settles", s.atRest(), String.format("%.0fms @144fps", ms));
    check("settles under 900ms", ms<900, String.format("%.0fms", ms));
    check("overshoots (snappy)", peak>1.0 && peak<1.15, String.format("peak %.4f", peak));
    check("lands exactly on target", Math.abs(s.value()-1.0)<1e-9, "value==1.0");

    Spring sm = Spring.smooth(0).target(1);
    double p2=0; int f2=0;
    while(!sm.atRest() && f2<2000){ sm.update(1.0/144.0); p2=Math.max(p2,sm.value()); f2++; }
    check("smooth does NOT overshoot", p2<=1.0001, String.format("peak %.5f", p2));

    // framerate independence
    double[] results=new double[3]; int[] rates={60,144,240};
    for(int i=0;i<3;i++){
      Spring x=Spring.snappy(0).target(1); double dt=1.0/rates[i];
      // Run to EXACTLY 0.1s of simulated time; clamp the final partial frame,
      // otherwise 144fps overshoots the budget by one whole frame.
      double t=0; while(t<0.1-1e-12){ double step=Math.min(dt,0.1-t); x.update(step); t+=step; }
      results[i]=x.value();
    }
    double spread=Math.max(results[0],Math.max(results[1],results[2]))
                 -Math.min(results[0],Math.min(results[1],results[2]));
    check("framerate independent", spread<0.01,
      String.format("60/144/240fps spread %.5f", spread));

    // interruption continuity - the whole reason we use springs
    Spring i1=Spring.snappy(0).target(1);
    for(int k=0;k<10;k++) i1.update(1.0/144.0);
    double vBefore=i1.velocity();
    i1.target(0);
    i1.update(1.0/144.0);
    check("velocity carries across retarget", Math.abs(i1.velocity()-vBefore)<50 && vBefore>0,
      String.format("v %.1f preserved", vBefore));

    // reduced motion
    Spring.reducedMotion=true;
    Spring rm=Spring.snappy(0).target(1); rm.update(1.0/144.0);
    check("reduced motion snaps instantly", rm.value()==1.0, "1 frame -> target");
    Spring.reducedMotion=false;

    // alt-tab stall clamp
    Spring st=Spring.snappy(0).target(1);
    st.update(5.0);
    check("clamps huge dt (alt-tab)", st.value()<=1.15 && !Double.isNaN(st.value()),
      String.format("5s frame -> %.4f, stable", st.value()));

    // ---- EASING ----
    System.out.println("\nEASING CURVES");
    check("expoOut bounds", Ease.expoOut(0)==0.0 && Ease.expoOut(1)==1.0, "0->0, 1->1");
    check("expoOut front-loaded", Ease.expoOut(0.25)>0.8,
      String.format("t=.25 -> %.3f", Ease.expoOut(0.25)));
    check("inOut symmetric", Math.abs(Ease.inOut(0.5)-0.5)<1e-9, "t=.5 -> 0.5");
    check("clamps out of range", Ease.expoOut(-3)==0.0 && Ease.quartOut(9)==1.0, "guarded");

    // ---- MONOCHROME LOCK ----
    System.out.println("\nMONOCHROME LOCK");
    int violations=0;
    for(Theme.Variant v : Theme.Variant.values()){
      Theme.setVariant(v);
      for(boolean hc : new boolean[]{false,true}){
        Theme.setHighContrast(hc);
        for(int l=0;l<=255;l++){
          if(!Theme.isMonochrome(Theme.lum(l))) violations++;
        }
      }
    }
    check("all 2048 theme colours monochrome", violations==0,
      "4 variants x 2 contrast x 256 levels");
    Theme.setVariant(Theme.Variant.SUMI); Theme.setHighContrast(false);
    check("alpha preserves luminance", Theme.isMonochrome(Theme.alpha(Theme.PAPER,0.5)),
      "alpha() stays grey");
    check("lum clamps overflow", Theme.isMonochrome(Theme.lum(999)) && Theme.isMonochrome(Theme.lum(-99)),
      "no wraparound");
    Theme.setVariant(Theme.Variant.YAMI);
    int y=Theme.lum(0x8C), sumiRef;
    Theme.setVariant(Theme.Variant.SUMI); sumiRef=Theme.lum(0x8C);
    check("variants alter contrast", y!=sumiRef, "YAMI != SUMI at same input");

    System.out.printf("%n\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500%n");
    System.out.printf("  %d passed, %d failed%n%n", pass, fail);
    if(fail>0) System.exit(1);
  }
}
