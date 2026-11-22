// Fails if a translation key used in the code is missing, or if en/fr differ.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const flat = (o, p = "") =>
  Object.entries(o).flatMap(([k, v]) => (typeof v === "object" ? flat(v, `${p}${k}.`) : [`${p}${k}`]));
const en = new Set(flat(JSON.parse(readFileSync("src/i18n/en.json", "utf8"))));
const fr = new Set(flat(JSON.parse(readFileSync("src/i18n/fr.json", "utf8"))));
const files = (d) => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? files(join(d, f)) : [join(d, f)]));
const used = new Set();
for (const f of [...files("pages"), ...files("src")].filter((f) => /\.tsx?$/.test(f))) {
  for (const m of readFileSync(f, "utf8").matchAll(/\bt\(\s*"([a-zA-Z0-9_.]+)"/g)) used.add(m[1]);
}
const has = (set, k) => set.has(k) || set.has(`${k}_one`);
let bad = 0;
for (const k of used) for (const [n, s] of [["en", en], ["fr", fr]]) if (!has(s, k)) (bad++, console.log(`missing ${n}: ${k}`));
for (const k of en) if (!fr.has(k)) (bad++, console.log(`fr lacks ${k}`));
for (const k of fr) if (!en.has(k)) (bad++, console.log(`en lacks ${k}`));
console.log(bad ? `${bad} problem(s)` : `i18n ok (${used.size} static keys used)`);
process.exit(bad ? 1 : 0);
