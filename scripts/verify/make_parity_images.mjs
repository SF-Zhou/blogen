#!/usr/bin/env node
/**
 * Capture raw evidence images for docs/parity/ (dev-only).
 *
 * Produces, from the frozen Node baseline and the current blogen candidate:
 *   - element screenshots of the first Mermaid sequence diagram (light + dark)
 *     for docs/parity/mermaid-static-vs-baseline.* comparison,
 *   - fixed-height (1800 CSS px) screenshots of representative pages for
 *     docs/parity/*.jpg.
 *
 * Outputs raw PNGs + raw-manifest.json to --out (default
 * scripts/verify/.cache/parity-raw). Compose/resize them into docs/parity/ with
 * `make_parity_images.py`.
 *
 * Usage:
 *   node scripts/verify/make_parity_images.mjs
 *   node scripts/verify/make_parity_images.mjs --baseline DIR --candidate DIR
 *
 * Exit codes: 0 = ok, 2 = usage/environment error.
 */

import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '../..');
const DEFAULT_BASELINE = path.join(SCRIPT_DIR, '.cache', 'baseline', 'public');
const DEFAULT_CANDIDATE = path.resolve(REPO_ROOT, '../blog/public');
const DEFAULT_OUT = path.join(SCRIPT_DIR, '.cache', 'parity-raw');
const DEFAULT_BLOG = path.resolve(REPO_ROOT, '../blog');

const VIEWPORT = { width: 1100, height: 900 };
// Article whose first Mermaid fence is a `sequenceDiagram` (the docs evidence).
const MERMAID_PAGE = 'rdma/rdma_buffer_safe_reuse.html';
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function parseArgs(argv) {
  const options = {
    baseline: DEFAULT_BASELINE,
    candidate: DEFAULT_CANDIDATE,
    out: DEFAULT_OUT,
    blogDir: DEFAULT_BLOG,
    blogDirExplicit: false,
    maxHeight: 6000,
    pageHeight: 1800,
  };
  const args = argv.slice();
  const takeValue = (index, name) => {
    if (args[index + 1] === undefined) throw new Error(`${name} needs a value`);
    return args[index + 1];
  };
  for (let i = 0; i < args.length; i += 1) {
    switch (args[i]) {
      case '--baseline': options.baseline = takeValue(i, '--baseline'); i += 1; break;
      case '--candidate': options.candidate = takeValue(i, '--candidate'); i += 1; break;
      case '--out': options.out = takeValue(i, '--out'); i += 1; break;
      case '--blog-dir':
        options.blogDir = takeValue(i, '--blog-dir');
        options.blogDirExplicit = true;
        i += 1;
        break;
      case '--max-height': options.maxHeight = parseInt(takeValue(i, '--max-height'), 10); i += 1; break;
      case '--page-height': options.pageHeight = parseInt(takeValue(i, '--page-height'), 10); i += 1; break;
      case '-h': case '--help':
        console.log('See the comment block at the top of make_parity_images.mjs for usage.');
        process.exit(0);
        break;
      default: throw new Error(`unknown argument: ${args[i]}`);
    }
  }
  for (const [name, dir] of [['baseline', options.baseline], ['candidate', options.candidate]]) {
    if (!fs.existsSync(path.join(dir, 'index.html'))) {
      throw new Error(`${name} site has no index.html: ${dir}`);
    }
  }
  options.baseline = path.resolve(options.baseline);
  options.candidate = path.resolve(options.candidate);
  options.out = path.resolve(options.out);
  return options;
}

function findPuppeteerRoot(options) {
  const candidates = options.blogDirExplicit
    ? [options.blogDir]
    : [options.blogDir, path.join(SCRIPT_DIR, '.cache', 'legacy-deps')];
  for (const candidate of candidates) {
    if (fs.existsSync(path.join(candidate, 'node_modules', 'puppeteer', 'package.json'))) {
      return candidate;
    }
  }
  return null;
}

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function startServer(siteDir) {
  const port = await freePort();
  const child = spawn('python3', ['-m', 'http.server', String(port), '--bind', '127.0.0.1'], {
    cwd: siteDir,
    stdio: ['ignore', 'ignore', 'ignore'],
  });
  const baseUrl = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error('python3 -m http.server exited early');
    try {
      const response = await fetch(`${baseUrl}/index.html`);
      if (response.ok) return { child, baseUrl };
    } catch {
      /* not up yet */
    }
    await sleep(100);
  }
  child.kill('SIGKILL');
  throw new Error('static server did not start');
}

function pageUrl(baseUrl, rel) {
  return `${baseUrl}/${rel.split('/').map((part) => encodeURIComponent(part)).join('/')}`;
}

function walkFiles(root) {
  const files = [];
  const stack = [''];
  while (stack.length) {
    const rel = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(path.join(root, rel), { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const childRel = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (entry.name === 'node_modules' || entry.name === '.git') continue;
        stack.push(childRel);
      } else if (entry.isFile()) files.push(childRel);
    }
  }
  return files.sort();
}

function pickMathCodePage(siteDir) {
  const htmlFiles = walkFiles(siteDir).filter((rel) => rel.endsWith('.html') && rel !== 'index.html');
  for (const rel of htmlFiles) {
    const html = fs.readFileSync(path.join(siteDir, rel), 'utf8');
    if (/<eq>|<eqn>/.test(html) && /<table>/.test(html) && /class="language-/.test(html)) {
      return rel;
    }
  }
  return null;
}

async function preparePage(browser, server, rel, scheme) {
  const page = await browser.newPage();
  await page.setViewport({ ...VIEWPORT, deviceScaleFactor: 2 });
  await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: scheme }]);
  await page.setRequestInterception(true);
  page.on('request', (request) => {
    const url = request.url();
    if (url.startsWith(server.baseUrl) || url.startsWith('data:') || url.startsWith('blob:')) {
      request.continue().catch(() => {});
    } else {
      request.abort().catch(() => {});
    }
  });
  await page.goto(pageUrl(server.baseUrl, rel), { waitUntil: 'networkidle0', timeout: 45000 });
  await page.evaluate(() => (document.fonts ? document.fonts.ready : Promise.resolve())).catch(() => {});
  await page.addStyleTag({
    content: '*,*::before,*::after{animation:none!important;transition:none!important;}'
      + 'html{scroll-behavior:auto!important;}*{caret-color:transparent!important;}',
  }).catch(() => {});
  await sleep(120);
  return page;
}

async function captureMermaidElement(browser, server, rel, scheme, outFile) {
  const page = await preparePage(browser, server, rel, scheme);
  try {
    const figure = await page.$('figure.mermaid');
    if (!figure) throw new Error(`no figure.mermaid found in ${rel}`);
    const box = await figure.boundingBox();
    // Element screenshots scroll the element into view automatically.
    await figure.screenshot({ path: outFile, type: 'png' });
    return { file: outFile, width: box ? Math.round(box.width) : null, height: box ? Math.round(box.height) : null };
  } finally {
    await page.close().catch(() => {});
  }
}

async function captureFullPage(browser, server, rel, scheme, outFile, options) {
  // Fixed-height page crops keep docs/parity small; the default 1800 CSS px at
  // deviceScaleFactor 2 resized to 700 px wide gives the same geometry as the
  // earlier docs screenshots (700 px wide JPEGs).
  const page = await preparePage(browser, server, rel, scheme);
  try {
    const dims = await page.evaluate(() => ({
      width: Math.max(document.documentElement.scrollWidth, 1100),
      height: Math.max(
        document.body ? document.body.scrollHeight : 0,
        document.documentElement.scrollHeight,
      ),
    }));
    const width = Math.min(Math.max(VIEWPORT.width, dims.width), 1400);
    const cappedHeight = Math.min(dims.height || VIEWPORT.height, options.pageHeight, options.maxHeight);
    const height = Math.max(VIEWPORT.height, cappedHeight);
    await page.screenshot({
      path: outFile,
      clip: { x: 0, y: 0, width, height },
      captureBeyondViewport: true,
    });
    return { file: outFile, width, height, capped: dims.height > height };
  } finally {
    await page.close().catch(() => {});
  }
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`make_parity_images: ${error.message}`);
    return 2;
  }
  const puppeteerRoot = findPuppeteerRoot(options);
  if (!puppeteerRoot) {
    console.error('make_parity_images: puppeteer not found; run '
      + '`scripts/verify/make_baseline.sh --bootstrap-node-modules` or pass --blog-dir');
    return 2;
  }
  const mathCodePage = pickMathCodePage(options.candidate);
  if (!mathCodePage) {
    console.error('make_parity_images: no article with math+code+table found in the candidate');
    return 2;
  }
  fs.rmSync(options.out, { recursive: true, force: true });
  fs.mkdirSync(options.out, { recursive: true });

  const requireFrom = createRequire(path.join(puppeteerRoot, 'node_modules', '__blogen_verify__.js'));
  const puppeteer = requireFrom('puppeteer');
  const servers = [];
  const manifest = {
    schema_version: 1,
    baseline: options.baseline,
    candidate: options.candidate,
    mermaid_page: MERMAID_PAGE,
    math_code_page: mathCodePage,
    captures: {},
  };
  const started = Date.now();
  try {
    const browser = await puppeteer.launch({
      headless: 'shell',
      args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
    });
    try {
      const baselineServer = await startServer(options.baseline);
      servers.push(baselineServer);
      const candidateServer = await startServer(options.candidate);
      servers.push(candidateServer);

      for (const scheme of ['light', 'dark']) {
        for (const [label, server] of [['baseline', baselineServer], ['candidate', candidateServer]]) {
          const out = path.join(options.out, `mermaid-${label}-${scheme}.png`);
          manifest.captures[`mermaid_${label}_${scheme}`] = await captureMermaidElement(
            browser, server, MERMAID_PAGE, scheme, out);
          console.log(`  captured ${path.basename(out)}`);
        }
      }

      const pageCaptures = [
        ['index-light', 'index.html', 'light'],
        ['math-code-light', mathCodePage, 'light'],
        ['mermaid-dark', MERMAID_PAGE, 'dark'],
      ];
      for (const [name, rel, scheme] of pageCaptures) {
        const out = path.join(options.out, `page-${name}.png`);
        manifest.captures[`page_${name}`] = {
          page: rel,
          scheme,
          ...(await captureFullPage(browser, candidateServer, rel, scheme, out, options)),
        };
        console.log(`  captured ${path.basename(out)} (${rel}, ${scheme})`);
      }

      manifest.duration_seconds = Math.round((Date.now() - started) / 100) / 10;
      fs.writeFileSync(path.join(options.out, 'raw-manifest.json'),
        `${JSON.stringify(manifest, null, 2)}\n`);
      console.log(`make_parity_images: raw evidence in ${options.out}`);
    } finally {
      await browser.close().catch(() => {});
    }
  } catch (error) {
    console.error(`make_parity_images: ${error.stack || error.message}`);
    return 2;
  } finally {
    for (const server of servers) {
      if (server.child.exitCode === null) server.child.kill('SIGTERM');
    }
  }
  return 0;
}

main()
  .then((code) => { process.exitCode = code; })
  .catch((error) => {
    console.error(`make_parity_images: ${error.stack || error.message}`);
    process.exitCode = 2;
  });
