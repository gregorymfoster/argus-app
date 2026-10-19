// Turns dist-single/index.html into a page body for the Artifact tool
// (it adds its own doctype/head skeleton). Output: out/argus-prototype.html
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';

const s = readFileSync(new URL('../dist-single/index.html', import.meta.url), 'utf8');
const head = s.match(/<head>([\s\S]*?)<\/head>/)[1].replace(/<meta charset[^>]*>|<meta name="viewport"[^>]*>/g, '');
const body = s.match(/<body>([\s\S]*?)<\/body>/)[1];
mkdirSync(new URL('../out/', import.meta.url), { recursive: true });
writeFileSync(new URL('../out/argus-prototype.html', import.meta.url), `${head.trim()}\n${body.trim()}\n`);
console.log('Wrote out/argus-prototype.html');
