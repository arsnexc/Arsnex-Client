package dev.arsex.mod.config;

import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.ModuleManager;
import dev.arsex.mod.module.Setting;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Reads and writes the module config the launcher also understands.
 *
 * Deliberately hand-rolled JSON rather than Gson: the schema is flat
 * (string -> string), and this keeps the mod free of any dependency the
 * launcher would also have to agree on. Writes go through a temp file and an
 * atomic move so a crash mid-save cannot leave a truncated config.
 */
public final class ConfigIO {

    private final Path file;

    public ConfigIO(Path file) {
        this.file = file;
    }

    // ------------------------------------------------------------------ save

    public void save(ModuleManager mm) throws IOException {
        StringBuilder sb = new StringBuilder();
        sb.append("{\n  \"version\": 1,\n  \"modules\": {\n");

        List<Module> all = new ArrayList<>(mm.all());
        for (int i = 0; i < all.size(); i++) {
            Module m = all.get(i);
            sb.append("    \"").append(esc(m.id)).append("\": {\n");
            sb.append("      \"enabled\": ").append(m.isEnabled()).append(",\n");
            sb.append("      \"keybind\": ").append(m.getKeybind()).append(",\n");
            sb.append("      \"settings\": {");

            List<Setting<?>> ss = m.settings();
            for (int j = 0; j < ss.size(); j++) {
                Setting<?> s = ss.get(j);
                sb.append("\n        \"").append(esc(s.name)).append("\": \"")
                  .append(esc(s.serialize())).append("\"");
                if (j < ss.size() - 1) sb.append(",");
            }
            if (!ss.isEmpty()) sb.append("\n      ");
            sb.append("}\n    }");
            if (i < all.size() - 1) sb.append(",");
            sb.append("\n");
        }
        sb.append("  }\n}\n");

        Path parent = file.getParent();
        if (parent != null) Files.createDirectories(parent);
        Path tmp = file.resolveSibling(file.getFileName() + ".tmp");
        Files.writeString(tmp, sb.toString(), StandardCharsets.UTF_8);
        try {
            Files.move(tmp, file, StandardCopyOption.REPLACE_EXISTING,
                    StandardCopyOption.ATOMIC_MOVE);
        } catch (java.nio.file.AtomicMoveNotSupportedException e) {
            Files.move(tmp, file, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    // ------------------------------------------------------------------ load

    public void load(ModuleManager mm) throws IOException {
        if (!Files.exists(file)) return;
        String raw = Files.readString(file, StandardCharsets.UTF_8);
        Map<String, ModuleState> states = parse(raw);

        for (Module m : mm.all()) {
            ModuleState st = states.get(m.id);
            if (st == null) continue;
            m.setKeybind(st.keybind);
            for (Setting<?> s : m.settings()) {
                String v = st.settings.get(s.name);
                if (v != null) s.deserialize(v);
            }
            // Settings first, then enable — so onEnable() sees final values.
            m.setEnabled(st.enabled);
        }
        mm.reindexKeys();
    }

    public static final class ModuleState {
        public boolean enabled;
        public int keybind = -1;
        public final Map<String, String> settings = new LinkedHashMap<>();
    }

    /**
     * Minimal recursive-descent parse of the exact shape save() emits.
     * Unknown keys are skipped; a malformed file yields whatever parsed cleanly
     * before the problem, so a hand-edit typo degrades instead of wiping config.
     */
    static Map<String, ModuleState> parse(String s) {
        Map<String, ModuleState> out = new LinkedHashMap<>();
        int mods = s.indexOf("\"modules\"");
        if (mods < 0) return out;
        int i = s.indexOf('{', mods);
        if (i < 0) return out;
        i++;

        while (i < s.length()) {
            i = skipWs(s, i);
            if (i >= s.length() || s.charAt(i) == '}') break;
            if (s.charAt(i) == ',') { i++; continue; }
            if (s.charAt(i) != '"') break;

            int[] key = readString(s, i);
            String id = s.substring(i + 1, key[0]);
            i = key[1];
            i = skipWs(s, i);
            if (i >= s.length() || s.charAt(i) != ':') break;
            i = skipWs(s, i + 1);
            if (i >= s.length() || s.charAt(i) != '{') break;

            int end = matchBrace(s, i);
            if (end < 0) break;
            out.put(unesc(id), parseModule(s.substring(i, end + 1)));
            i = end + 1;
        }
        return out;
    }

    private static ModuleState parseModule(String body) {
        ModuleState st = new ModuleState();
        st.enabled = readBool(body, "enabled");
        st.keybind = readInt(body, "keybind", -1);

        int sIdx = body.indexOf("\"settings\"");
        if (sIdx >= 0) {
            int open = body.indexOf('{', sIdx);
            int close = matchBrace(body, open);
            if (open >= 0 && close > open) {
                String inner = body.substring(open + 1, close);
                int i = 0;
                while (i < inner.length()) {
                    i = skipWs(inner, i);
                    if (i >= inner.length() || inner.charAt(i) != '"') { i++; continue; }
                    int[] k = readString(inner, i);
                    String name = inner.substring(i + 1, k[0]);
                    i = skipWs(inner, k[1]);
                    if (i >= inner.length() || inner.charAt(i) != ':') { i++; continue; }
                    i = skipWs(inner, i + 1);
                    if (i >= inner.length() || inner.charAt(i) != '"') { i++; continue; }
                    int[] v = readString(inner, i);
                    st.settings.put(unesc(name), unesc(inner.substring(i + 1, v[0])));
                    i = v[1];
                }
            }
        }
        return st;
    }

    // --------------------------------------------------------------- helpers

    private static int skipWs(String s, int i) {
        while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
        return i;
    }

    /** Returns {indexOfClosingQuote, indexAfterClosingQuote}, honouring escapes. */
    private static int[] readString(String s, int openQuote) {
        int i = openQuote + 1;
        while (i < s.length()) {
            char c = s.charAt(i);
            if (c == '\\') { i += 2; continue; }
            if (c == '"') return new int[]{i, i + 1};
            i++;
        }
        return new int[]{s.length(), s.length()};
    }

    /** Brace matcher that ignores braces inside string literals. */
    private static int matchBrace(String s, int open) {
        if (open < 0 || open >= s.length() || s.charAt(open) != '{') return -1;
        int depth = 0;
        boolean inStr = false;
        for (int i = open; i < s.length(); i++) {
            char c = s.charAt(i);
            if (inStr) {
                if (c == '\\') i++;
                else if (c == '"') inStr = false;
                continue;
            }
            if (c == '"') inStr = true;
            else if (c == '{') depth++;
            else if (c == '}' && --depth == 0) return i;
        }
        return -1;
    }

    private static boolean readBool(String body, String key) {
        int i = body.indexOf("\"" + key + "\"");
        if (i < 0) return false;
        int c = body.indexOf(':', i);
        if (c < 0) return false;
        return body.startsWith("true", skipWs(body, c + 1));
    }

    private static int readInt(String body, String key, int def) {
        int i = body.indexOf("\"" + key + "\"");
        if (i < 0) return def;
        int c = skipWs(body, body.indexOf(':', i) + 1);
        int j = c;
        if (j < body.length() && body.charAt(j) == '-') j++;
        while (j < body.length() && Character.isDigit(body.charAt(j))) j++;
        try { return Integer.parseInt(body.substring(c, j)); }
        catch (RuntimeException e) { return def; }
    }

    static String esc(String s) {
        StringBuilder b = new StringBuilder(s.length() + 8);
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"'  -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                default   -> b.append(c);
            }
        }
        return b.toString();
    }

    static String unesc(String s) {
        StringBuilder b = new StringBuilder(s.length());
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '\\' && i + 1 < s.length()) {
                char n = s.charAt(++i);
                switch (n) {
                    case 'n'  -> b.append('\n');
                    case 'r'  -> b.append('\r');
                    case 't'  -> b.append('\t');
                    case '"'  -> b.append('"');
                    case '\\' -> b.append('\\');
                    default   -> b.append(n);
                }
            } else b.append(c);
        }
        return b.toString();
    }
}
