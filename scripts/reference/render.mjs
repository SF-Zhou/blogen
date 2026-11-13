#!/usr/bin/env node
// Dev-only CLI around the reference pipeline. See README.md.
//
//   node render.mjs input.md                 # HTML to stdout
//   node render.mjs input.md --out out.html  # HTML to file
//   node render.mjs --corpus DIR --out-dir OUT_DIR [--mermaid MODE]
//   node render.mjs --version
//
// --mermaid MODE controls mermaid fences, matching blogen's `[markdown]
// mermaid` setting:
//   none         leave the raw `<pre class="mermaid-placeholder">N</pre>` marker
//                (the default, i.e. what the reference renderer itself emits)
//   placeholder  replace it by the deterministic marker figure blogen emits in
//                `mermaid = "placeholder"` mode (identical md5-based ids)
// `client` is not supported here: server-side SVG rendering needs puppeteer.
import { promises as fs } from 'fs';
import { join, dirname, basename, resolve } from 'path';
import { render, render_mermaid_definitions } from './marked.js';

async function walk(dir, ext, out = []) {
  for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) await walk(p, ext, out);
    else if (entry.isFile() && entry.name.endsWith(ext)) out.push(p);
  }
  return out.sort();
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--version')) {
    console.log('blogen-reference-renderer (markdown-it 14.3.0, katex 0.18.1, prismjs 1.30.0)');
    return;
  }
  const get = (flag) => {
    const i = args.indexOf(flag);
    return i >= 0 ? args[i + 1] : undefined;
  };
  const corpus = get('--corpus');
  const outDir = get('--out-dir');
  const mermaid = get('--mermaid');
  const input = args.find((a) => !a.startsWith('--') && args[args.indexOf(a) - 1] !== '--out-dir' && args[args.indexOf(a) - 1] !== '--corpus');
  const out = get('--out');

  if (corpus && outDir) {
    const files = await walk(corpus, '.md');
    for (const file of files) {
      const rel = file.slice(corpus.length + 1).replace(/\.md$/, '.html');
      const dest = join(outDir, rel);
      await fs.mkdir(dirname(dest), { recursive: true });
      const markdown = await fs.readFile(file, 'utf8');
      await fs.writeFile(dest, render(markdown, { mermaid }));
    }
    console.log(`rendered ${files.length} file(s) to ${outDir}`);
    return;
  }
  if (!input) {
    console.error('usage: node render.mjs <input.md> [--out file] | --corpus DIR --out-dir DIR');
    process.exit(2);
  }
  const markdown = await fs.readFile(resolve(input), 'utf8');
  const html = render(markdown, { mermaid });
  if (out) {
    await fs.mkdir(dirname(resolve(out)), { recursive: true });
    await fs.writeFile(resolve(out), html);
  } else {
    process.stdout.write(html);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
