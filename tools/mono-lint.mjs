#!/usr/bin/env node
/**
 * Arsex monochrome lint.
 *
 * Walks compiled output and fails the build if ANY colour literal has non-zero
 * saturation. This is what makes "pure black and white" a structural guarantee
 * instead of a design note somebody forgets in month four.
 *
 * Handles: #rgb #rgba #rrggbb #rrggbbaa, rgb()/rgba(), hsl()/hsla(),
 * and named CSS colours.
 *
 * NOTE: lives in tools/ not build/ — "build" is a conventional artefact
 * directory name and gets excluded by snapshot/ignore rules.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, extname } from "node:path";

const TOLERANCE = 0;          // channel spread allowed. zero means zero.
const EXT = new Set([".css", ".js", ".mjs", ".html", ".json", ".svg"]);

const GREY_NAMES = new Set([
  "black","white","transparent","currentcolor","inherit","initial","unset",
  "gray","grey","dimgray","dimgrey","darkgray","darkgrey","lightgray",
  "lightgrey","silver","gainsboro","whitesmoke","snow","ivory","none",
]);

const NAMED = /\b(aliceblue|antiquewhite|aqua|aquamarine|azure|beige|bisque|blanchedalmond|blue|blueviolet|brown|burlywood|cadetblue|chartreuse|chocolate|coral|cornflowerblue|cornsilk|crimson|cyan|darkblue|darkcyan|darkgoldenrod|darkgreen|darkkhaki|darkmagenta|darkolivegreen|darkorange|darkorchid|darkred|darksalmon|darkseagreen|darkslateblue|darkturquoise|darkviolet|deeppink|deepskyblue|dodgerblue|firebrick|floralwhite|forestgreen|fuchsia|gold|goldenrod|green|greenyellow|honeydew|hotpink|indianred|indigo|khaki|lavender|lawngreen|lemonchiffon|lightblue|lightcoral|lightcyan|lightgreen|lightpink|lightsalmon|lightseagreen|lightskyblue|lightsteelblue|lightyellow|lime|limegreen|linen|magenta|maroon|mediumblue|mediumorchid|mediumpurple|mediumseagreen|mediumslateblue|mediumspringgreen|mediumturquoise|mediumvioletred|midnightblue|mintcream|mistyrose|moccasin|navajowhite|navy|oldlace|olive|olivedrab|orange|orangered|orchid|palegoldenrod|palegreen|paleturquoise|palevioletred|papayawhip|peachpuff|peru|pink|plum|powderblue|purple|rebeccapurple|red|rosybrown|royalblue|saddlebrown|salmon|sandybrown|seagreen|seashell|sienna|skyblue|slateblue|springgreen|steelblue|tan|teal|thistle|tomato|turquoise|violet|wheat|yellow|yellowgreen)\b/gi;

const violations = [];

function checkRGB(r, g, b, ctx, file, line) {
  const spread = Math.max(r, g, b) - Math.min(r, g, b);
  if (spread > TOLERANCE) {
    violations.push({ file, line, ctx, detail: `rgb(${r},${g},${b}) spread ${spread}` });
  }
}

function scanText(text, file) {
  text.split("\n").forEach((lineText, i) => {
    const line = i + 1;
    for (const m of lineText.matchAll(/#([0-9a-f]{3,8})\b/gi)) {
      let h = m[1];
      if (h.length === 4) h = h.slice(0, 3);
      if (h.length === 8) h = h.slice(0, 6);
      if (h.length === 3) h = h.split("").map(c => c + c).join("");
      if (h.length !== 6) continue;
      checkRGB(parseInt(h.slice(0,2),16), parseInt(h.slice(2,4),16),
               parseInt(h.slice(4,6),16), m[0], file, line);
    }
    for (const m of lineText.matchAll(/rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/gi)) {
      checkRGB(+m[1], +m[2], +m[3], m[0], file, line);
    }
    for (const m of lineText.matchAll(/hsla?\(\s*[\d.]+(?:deg)?[\s,]+([\d.]+)%/gi)) {
      if (parseFloat(m[1]) > 0) {
        violations.push({ file, line, ctx: m[0], detail: `saturation ${m[1]}%` });
      }
    }
    for (const m of lineText.matchAll(NAMED)) {
      if (!GREY_NAMES.has(m[0].toLowerCase())) {
        violations.push({ file, line, ctx: m[0], detail: "named colour" });
      }
    }
  });
}

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const st = statSync(p);
    if (st.isDirectory()) walk(p);
    else if (EXT.has(extname(p))) scanText(readFileSync(p, "utf8"), p);
  }
}

const target = process.argv[2];
if (!target) { console.error("usage: mono-lint.mjs <dir>"); process.exit(2); }
walk(target);

if (violations.length) {
  console.error(`\n  MONOCHROME LINT FAILED — ${violations.length} violation(s)\n`);
  for (const v of violations.slice(0, 40)) {
    console.error(`    ${v.file}:${v.line}  ${v.ctx}  (${v.detail})`);
  }
  if (violations.length > 40) console.error(`    ... and ${violations.length - 40} more`);
  console.error("");
  process.exit(1);
}
console.log("  monochrome lint passed — zero saturation in output");
