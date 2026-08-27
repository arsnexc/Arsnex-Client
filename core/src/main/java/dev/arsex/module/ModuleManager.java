package dev.arsex.module;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.*;
import java.util.stream.Collectors;

/**
 * Central registry. Lookups by id and by keybind are O(1) because both run on
 * hot paths (config load, key event).
 */
public final class ModuleManager {
    public static final Logger LOG = LoggerFactory.getLogger("Arsex");

    private final Map<String, Module> byId = new LinkedHashMap<>();
    private final Map<Integer, List<Module>> byKey = new HashMap<>();

    public void register(Module m) {
        if (byId.putIfAbsent(m.id, m) != null) {
            throw new IllegalStateException("duplicate module id: " + m.id);
        }
    }

    public void registerAll(Module... ms) { for (Module m : ms) register(m); }

    public Optional<Module> get(String id) {
        return Optional.ofNullable(byId.get(id));
    }

    public Collection<Module> all() { return byId.values(); }

    public List<Module> byCategory(Category c) {
        return byId.values().stream()
                .filter(m -> m.category == c)
                .collect(Collectors.toList());
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

    public void rebuildKeybinds() {
        byKey.clear();
        for (Module m : byId.values()) {
            if (m.getKeybind() >= 0) {
                byKey.computeIfAbsent(m.getKeybind(), k -> new ArrayList<>()).add(m);
            }
        }
    }

    /** Conflict detection — surfaced in the keybind UI, not silently allowed. */
    public Map<Integer, List<Module>> conflicts() {
        rebuildKeybinds();
        return byKey.entrySet().stream()
                .filter(e -> e.getValue().size() > 1)
                .collect(Collectors.toMap(Map.Entry::getKey, Map.Entry::getValue));
    }

    public void onKey(int key) {
        List<Module> hit = byKey.get(key);
        if (hit != null) hit.forEach(Module::toggle);
    }

    public void onTick() {
        for (Module m : byId.values()) {
            if (!m.isEnabled()) continue;
            try { m.onTick(); }
            catch (Throwable t) { failSafe(m, t); }
        }
    }

    public void onRender(float delta) {
        for (Module m : byId.values()) {
            if (!m.isEnabled()) continue;
            try { m.onRender(delta); }
            catch (Throwable t) { failSafe(m, t); }
        }
    }

    private void failSafe(Module m, Throwable t) {
        LOG.error("Module '{}' threw; disabling to protect the session", m.id, t);
        m.setEnabled(false);
    }

    public int enabledCount() {
        return (int) byId.values().stream().filter(Module::isEnabled).count();
    }
}
