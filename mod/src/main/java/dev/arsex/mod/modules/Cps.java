package dev.arsex.mod.modules;

import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.Setting;

import java.util.ArrayDeque;
import java.util.Deque;

/**
 * Clicks per second.
 *
 * A sliding one-second window over click timestamps. Old entries are evicted
 * from the head, so this is O(1) amortised and never grows unbounded even
 * under a drag-click.
 */
public final class Cps extends Module {
    private final Setting.Bool right = register(new Setting.Bool(
            "Right Click", "Also count right clicks.", true));

    private static final int MAX = 512;   // hard cap: a butterfly-clicker cannot OOM us
    private final Deque<Long> left = new ArrayDeque<>();
    private final Deque<Long> rightQ = new ArrayDeque<>();

    public Cps() {
        super("cps", "CPS Counter", "Sliding-window clicks per second", Category.HUD);
    }

    public void onLeftClick()  { record(left); }
    public void onRightClick() { if (right.get()) record(rightQ); }

    private void record(Deque<Long> q) {
        long now = System.currentTimeMillis();
        if (q.size() >= MAX) q.pollFirst();
        q.addLast(now);
    }

    private int count(Deque<Long> q) {
        long cutoff = System.currentTimeMillis() - 1000L;
        while (!q.isEmpty() && q.peekFirst() < cutoff) q.pollFirst();
        return q.size();
    }

    public int leftCps()  { return count(left); }
    public int rightCps() { return count(rightQ); }

    public String display() {
        return right.get() ? leftCps() + " | " + rightCps() : String.valueOf(leftCps());
    }

    @Override protected void onDisable() {
        left.clear();
        rightQ.clear();
    }
}
