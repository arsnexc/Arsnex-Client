package dev.arsex.mod.module;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.stream.Collectors;

/**
 * Central registry. Lookup by id and by keybind are both O(1): the first runs
 * on config load, the second on every key event.
 */
public final class ModuleManager {
    public static final Logger LOG = LoggerFactory.getLogger("Arsex");

    private final Map<String, Module> byId = new LinkedHashMap<>();
    private final Map<Integer, List<Module>> byKey = new HashMap<>();

    public void register(Module m) {
        if (byId.putIfAbsent(m.id, m) != null) {
            throw new IllegalStateException("duplicate module id: " + m.id);
        }
        reindexKeys();
    }

    public void registerAll(Module... ms) {
        for (Module m : ms) {
            if (byId.putIfAbsent(m.id, m) != null) {
                throw new IllegalStateException("duplicate module id: " + m.id);
            }
        }
        reindexKeys();
    }

    /** Rebuilt whenever a bind changes; keeps the key event path allocation-free. */
    public void reindexKeys() {
        byKey.clear();
        for (Module m : byId.values()) {
            if (m.getKeybind() > 0) {
                byKey.computeIfAbsent(m.getKeybind(), k -> new ArrayList<>()).add(m);
            }
        }
    }

    public Optional<Module> get(String id)   { return Optional.ofNullable(byId.get(id)); }
    public Collection<Module> all()          { return byId.values(); }

    public List<Module> byCategory(Category c) {
        return byId.values().stream().filter(m -> m.category == c).collect(Collectors.toList());
    }

    public List<Module> enabled() {
        return byId.values().stream().filter(Module::isEnabled).collect(Collectors.toList());
    }

    /** Returns how many modules fired, so the caller can swallow the key if > 0. */
    public int onKey(int key) {
        List<Module> hit = byKey.get(key);
        if (hit == null || hit.isEmpty()) return 0;
        for (Module m : hit) m.toggle();
        return hit.size();
    }

    public void onTick() {
        for (Module m : byId.values()) {
            if (!m.isEnabled()) continue;
            try {
                m.onTick();
            } catch (Throwable t) {
                // One misbehaving module must not break the tick loop for the rest.
                LOG.error("module {} threw during tick, disabling", m.id, t);
                m.setEnabled(false);
            }
        }
    }

    /** Powers the GUI search bar. Prefix matches rank above substring matches. */
    public List<Module> search(String query) {
        if (query == null || query.isBlank()) return new ArrayList<>(byId.values());
        String q = query.toLowerCase(Locale.ROOT).trim();
        return byId.values().stream()
                .filter(m -> m.name.toLowerCase(Locale.ROOT).contains(q)
                          || m.id.contains(q)
                          || m.category.label.toLowerCase(Locale.ROOT).contains(q))
                .sorted(Comparator.comparingInt(m ->
                        m.name.toLowerCase(Locale.ROOT).startsWith(q) ? 0 : 1))
                .collect(Collectors.toList());
    }
}
