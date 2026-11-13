#!/usr/bin/env node
/**
 * Screenshot parity check for blogen.
 *
 * Captures full-page screenshots (1100px wide viewport, light + dark
 * `prefers-color-scheme`) of a representative set of pages from
 *
 *   - the reference baseline (scripts/verify/.cache/baseline/public), and
 *   - a blogen candidate site (when --candidate is given),
 *
 * then pixel-diffs baseline vs candidate with scripts/verify/pixdiff.py
 * (PIL + numpy, per-channel tolerance) and writes per-page diff PNGs plus a
 * JSON report under scripts/verify/.cache/screenshots/.
 *
 * Without --candidate this runs in "baseline-only" mode: it just captures the
 * baseline screenshots (useful while blogen cannot build the real blog yet).
 *
 * Mermaid pages are treated as "soft": the frozen Node baseline renders their
 * diagrams server-side with mermaid-cli (puppeteer), while blogen now inlines
 * build-time SVG produced by the pure-Rust `merman` renderer. The visual result
 * is close but not pixel-identical (Mermaid version, theme palette and text
 * measurement differ), which is an accepted trade-off; their real pixel-diff
 * numbers are captured and reported but do not fail the run. Everything else
 * fails when more than --threshold percent of pixels differ.
 *
 * Mermaid figures are fully static: generated pages contain inline
 * `<figure class="mermaid" data-mermaid-digest=…><div class="mermaid-light"><svg…
 * markup with no Mermaid JavaScript, so captures never wait for JS rendering.
 * `check_static_mermaid.mjs` verifies the JS-free property explicitly.
 *
 * Usage:
 *   node scripts/verify/screenshots.mjs                       # baseline only
 *   node scripts/verify/screenshots.mjs --candidate DIR       # capture + diff
 *   node scripts/verify/screenshots.mjs --pages index.html,rust/x.html --schemes light
 *
 * Options:
 *   --baseline DIR     baseline site root (default: .cache/baseline/public)
 *   --candidate DIR    candidate site root (contains index.html)
 *   --out DIR          output directory (default: .cache/screenshots)
 *   --pages LIST       comma-separated page paths relative to the site root
 *                      (default: index + representative pages detected from HTML)
 *   --schemes LIST     comma-separated: light,dark (default: light,dark)
 *   --blog-dir DIR     root whose node_modules provides puppeteer (the legacy
 *                      blog checkout, default: ../../../blog next to blogen;
 *                      falls back to .cache/legacy-deps when the blog deps are
 *                      gone after the migration)
 *   --max-height N     cap full-page height in px (default: 6000)
 *   --threshold N      hard-fail threshold in percent differing pixels (default: 0.5)
 *   --tol N            pixel per-channel tolerance 0-255 (default: 12)
 *   --json             print the machine-readable report to stdout
 *   --quiet            less progress output
 *
 * Exit codes: 0 = pass / baseline-only success, 1 = hard visual difference,
 * 2 = usage or environment error.
 */

import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '../..');
const DEFAULT_BLOG = path.resolve(REPO_ROOT, '../blog');
const DEFAULT_BASELINE = path.join(SCRIPT_DIR, '.cache/baseline/public');
const DEFAULT_OUT = path.join(SCRIPT_DIR, '.cache/screenshots');

const VIEWPORT = { width: 1100, height: 900 };
const SCHEMES = ['light', 'dark'];
const DEFAULT_SCHEMES = ['light', 'dark'];

const HELP = `See the comment block at the top of scripts/verify/screenshots.mjs for usage.`;

function parseArgs(argv) {
  const options = {
    baseline: DEFAULT_BASELINE,
    candidate: null,
    out: DEFAULT_OUT,
    pages: null,
    schemes: DEFAULT_SCHEMES.slice(),
    blogDir: DEFAULT_BLOG,
    blogDirExplicit: false,
    maxHeight: 6000,
    maxWidth: 1400,
    threshold: 0.5,
    tol: 12,
    json: false,
    quiet: false,
  };
  const args = argv.slice();
  const value = (flag) => {
    const i = args.indexOf(flag);
    return i >= 0 && i + 1 < args.length ? args[i + 1] : undefined;
  };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    const next = args[i + 1];
    const takeValue = (name) => {
      if (next === undefined) throw new Error(`${name} needs a value`);
      i += 1;
      return next;
    };
    switch (arg) {
      case '--baseline': options.baseline = takeValue(arg); break;
      case '--candidate': options.candidate = takeValue(arg); break;
      case '--out': options.out = takeValue(arg); break;
      case '--pages':
        options.pages = takeValue(arg).split(',').map((p) => p.trim()).filter(Boolean);
        break;
      case '--schemes':
        options.schemes = takeValue(arg).split(',').map((s) => s.trim()).filter(Boolean);
        for (const scheme of options.schemes) {
          if (!SCHEMES.includes(scheme)) throw new Error(`unknown scheme: ${scheme}`);
        }
        break;
      case '--blog-dir':
        options.blogDir = takeValue(arg);
        options.blogDirExplicit = true;
        break;
      case '--max-height': options.maxHeight = parseInt(takeValue(arg), 10); break;
      case '--max-width': options.maxWidth = parseInt(takeValue(arg), 10); break;
      case '--threshold': options.threshold = parseFloat(takeValue(arg)); break;
      case '--tol': options.tol = parseInt(takeValue(arg), 10); break;
      case '--json': options.json = true; break;
      case '--quiet': options.quiet = true; break;
      case '-h': case '--help': console.log(HELP); process.exit(0); break;
      default: throw new Error(`unknown argument: ${arg}`);
    }
  }
  options.baseline = path.resolve(options.baseline);
  if (options.candidate) options.candidate = path.resolve(options.candidate);
  options.out = path.resolve(options.out);
  options.blogDir = path.resolve(options.blogDir);
  return options;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const log = (options, message) => {
  if (!options.quiet) console.log(message);
};

// ---------------------------------------------------------------------------
// HTTP server for a site directory
// ---------------------------------------------------------------------------
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
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  child.stderr.on('data', (data) => {
    const text = data.toString();
    if (!/Serving HTTP|Error response|code 404|Broken pipe/.test(text)) {
      process.stderr.write(`http.server: ${text}`);
    }
  });
  const baseUrl = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error('python3 -m http.server exited early');
    try {
      const response = await fetch(`${baseUrl}/index.html`);
      if (response.ok) return { child, baseUrl, siteDir };
    } catch {
      /* not up yet */
    }
    await sleep(100);
  }
  child.kill('SIGKILL');
  throw new Error(`static server for ${siteDir} did not start`);
}

function stopServer(server) {
  if (server && server.child.exitCode === null) {
    server.child.kill('SIGTERM');
    setTimeout(() => {
      if (server.child.exitCode === null) server.child.kill('SIGKILL');
    }, 2000).unref();
  }
}

// ---------------------------------------------------------------------------
// Page discovery / selection
// ---------------------------------------------------------------------------
function walkFiles(root) {
  const files = [];
  const stack = [''];
  while (stack.length) {
    const rel = stack.pop();
    const full = path.join(root, rel);
    let entries;
    try {
      entries = fs.readdirSync(full, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const childRel = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (entry.name === 'dist' || entry.name === 'node_modules' || entry.name === '.git') continue;
        stack.push(childRel);
      } else if (entry.isFile()) {
        files.push(childRel);
      }
    }
  }
  return files.sort();
}

function readText(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function pageFeatures(siteDir, rel) {
  const html = readText(path.join(siteDir, rel));
  return {
    // Static build-time figures carry data-mermaid-digest; the older
    // client-side rendering mode only used the class. Match both.
    mermaid: /data-mermaid-digest|class="mermaid/.test(html),
    figure: /<figure tabindex=/.test(html),
    katex: /<eq>|<eqn>|class="katex/.test(html),
    table: /<table>/.test(html),
    hidden: rel.endsWith('.htm'),
    nested: rel.split('/').length > 1,
    video: /<video|<iframe/.test(html),
  };
}

function choosePages(siteDir, requested) {
  const files = walkFiles(siteDir);
  if (requested && requested.length) return requested.slice();
  const pages = ['index.html'];
  const candidates = files.filter((rel) => rel !== 'index.html'
    && (rel.endsWith('.html') || rel.endsWith('.htm')));
  const picked = new Set(pages);
  const plan = [
    ['mermaid', 2],
    ['figure', 1],
    ['katex', 1],
    ['table', 1],
    ['hidden', 1],
    ['nested', 1],
  ];
  for (const [feature, count] of plan) {
    let taken = 0;
    for (const rel of candidates) {
      if (taken >= count) break;
      if (picked.has(rel)) continue;
      if (pageFeatures(siteDir, rel)[feature]) {
        picked.add(rel);
        taken += 1;
      }
    }
  }
  // Fill to 8 pages in a deterministic order if the site is feature-poor.
  for (const rel of candidates) {
    if (picked.size >= 8) break;
    picked.add(rel);
  }
  return pages.concat([...picked].filter((rel) => rel !== 'index.html'));
}

// ---------------------------------------------------------------------------
// Puppeteer capture
// ---------------------------------------------------------------------------
function findPuppeteerRoot(options) {
  // Puppeteer lives in the legacy Node blog's node_modules. After the blog was
  // migrated those deps may only exist in the frozen bootstrap copy.
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

function loadPuppeteer(moduleRoot) {
  const anchor = path.join(moduleRoot, 'node_modules', '__blogen_verify_anchor__.js');
  const requireFrom = createRequire(anchor);
  return requireFrom('puppeteer');
}

function pageUrl(baseUrl, rel) {
  const encoded = rel.split('/').map((part) => encodeURIComponent(part)).join('/');
  return `${baseUrl}/${encoded}`;
}

function pageSlug(rel) {
  return rel.replace(/\.(html|htm)$/, '').replace(/[^A-Za-z0-9._-]+/g, '__') || 'index';
}

async function capturePage(browser, server, rel, scheme, outFile, options) {
  const page = await browser.newPage();
  const notes = [];
  let width = VIEWPORT.width;
  let height = VIEWPORT.height;
  let capped = false;
  const result = { page: rel, scheme, file: outFile, width, height, capped, notes };
  try {
    await page.setViewport({ ...VIEWPORT, deviceScaleFactor: 1 });
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
    const url = pageUrl(server.baseUrl, rel);
    try {
      await page.goto(url, { waitUntil: 'networkidle0', timeout: 45000 });
    } catch (error) {
      notes.push(`goto(${error.message.split('\n')[0]})`);
      await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 }).catch(() => {});
    }
    await page.evaluate(() => (document.fonts ? document.fonts.ready : Promise.resolve())).catch(() => {});
    // Mermaid figures are static inline SVG rendered at build time (no JS).
    // Record the diagram/SVG counts for the report and sanity-check them; never
    // wait for a client-side render.
    const mermaidInfo = await page.evaluate(() => ({
      figures: document.querySelectorAll('figure.mermaid, .mermaid').length,
      svgs: document.querySelectorAll('.mermaid svg, .mermaid-light svg, .mermaid-dark svg').length,
    })).catch(() => null);
    if (mermaidInfo) {
      result.mermaid = mermaidInfo;
      if (mermaidInfo.figures > 0 && mermaidInfo.svgs === 0) {
        notes.push('mermaid-figure-without-inline-svg');
      }
    }
    // Deterministic rendering: no animations/transitions/caret.
    await page.addStyleTag({
      content: '*,*::before,*::after{animation:none!important;transition:none!important;}'
        + 'html{scroll-behavior:auto!important;}*{caret-color:transparent!important;}',
    }).catch(() => {});
    await sleep(150);
    const dims = await page.evaluate(() => ({
      width: Math.max(document.documentElement.scrollWidth, 1100),
      height: Math.max(
        document.body ? document.body.scrollHeight : 0,
        document.documentElement.scrollHeight,
      ),
    })).catch(() => ({ width: VIEWPORT.width, height: VIEWPORT.height }));
    width = Math.min(Math.max(VIEWPORT.width, dims.width), options.maxWidth);
    height = Math.max(VIEWPORT.height, Math.min(dims.height || VIEWPORT.height, options.maxHeight));
    capped = (dims.height || 0) > options.maxHeight;
    await page.screenshot({
      path: outFile,
      clip: { x: 0, y: 0, width, height },
      captureBeyondViewport: true,
      optimizeForSpeed: false,
    });
    result.width = width;
    result.height = height;
    result.capped = capped;
    return result;
  } finally {
    await page.close().catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// Pixel diff via pixdiff.py
// ---------------------------------------------------------------------------
function runPixdiff(a, b, diffOut, tol) {
  const script = path.join(SCRIPT_DIR, 'pixdiff.py');
  return new Promise((resolve, reject) => {
    const child = spawn('python3', [
      script, '--a', a, '--b', b, '--tol', String(tol), '--out-diff', diffOut,
    ]);
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (data) => { stdout += data.toString(); });
    child.stderr.on('data', (data) => { stderr += data.toString(); });
    child.on('error', reject);
    child.on('close', (code) => {
      if (code !== 0) {
        reject(new Error(`pixdiff failed (${code}): ${stderr.trim()}`));
        return;
      }
      try {
        resolve(JSON.parse(stdout.trim().split('\n').pop()));
      } catch (error) {
        reject(new Error(`cannot parse pixdiff output: ${error.message}`));
      }
    });
  });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`screenshots: ${error.message}`);
    return 2;
  }
  for (const [label, dir] of [
    ['baseline site', options.baseline],
  ]) {
    if (!fs.existsSync(dir)) {
      console.error(`screenshots: ${label} not found: ${dir}`);
      return 2;
    }
  }
  if (options.candidate) {
    // Accept either a site root or a build dir containing public/index.html.
    if (!fs.existsSync(path.join(options.candidate, 'index.html'))
      && fs.existsSync(path.join(options.candidate, 'public', 'index.html'))) {
      options.candidate = path.join(options.candidate, 'public');
    }
    if (!fs.existsSync(path.join(options.candidate, 'index.html'))) {
      console.error(`screenshots: candidate has no index.html: ${options.candidate}`);
      return 2;
    }
  }

  const puppeteerRoot = findPuppeteerRoot(options);
  if (!puppeteerRoot) {
    console.error('screenshots: puppeteer not found. Looked in: '
      + (options.blogDirExplicit ? options.blogDir : `${options.blogDir}, ${path.join(SCRIPT_DIR, '.cache', 'legacy-deps')}`)
      + '. Re-install the frozen legacy deps with `scripts/verify/make_baseline.sh --bootstrap-node-modules` '
      + 'or pass --blog-dir <legacy-blog-checkout>.');
    return 2;
  }
  let puppeteer;
  try {
    puppeteer = loadPuppeteer(puppeteerRoot);
  } catch (error) {
    console.error(`screenshots: cannot load puppeteer from ${puppeteerRoot}/node_modules: ${error.message}`);
    return 2;
  }

  fs.mkdirSync(options.out, { recursive: true });
  const pages = choosePages(options.baseline, options.pages);
  if (options.candidate && options.pages) {
    // Pages requested explicitly must exist in the candidate as well.
  }
  const report = {
    schema_version: 1,
    mode: options.candidate ? 'diff' : 'baseline-only',
    viewport: VIEWPORT,
    schemes: options.schemes,
    threshold_pct: options.threshold,
    tolerance: options.tol,
    max_height: options.maxHeight,
    baseline_dir: options.baseline,
    candidate_dir: options.candidate,
    pages,
    captures: { baseline: [], candidate: [] },
    comparisons: [],
    soft_pages: [],
    summary: {},
  };

  log(options, `screenshots: ${options.candidate ? 'baseline vs candidate' : 'baseline only'}`
    + ` (${pages.length} page(s) x ${options.schemes.length} scheme(s), viewport ${VIEWPORT.width}x${VIEWPORT.height})`);
  log(options, `  baseline : ${options.baseline}`);
  if (options.candidate) log(options, `  candidate: ${options.candidate}`);

  const started = Date.now();
  let browser = null;
  const servers = [];
  let hardFailures = 0;
  try {
    browser = await puppeteer.launch({
      headless: 'shell',
      args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
    });
    servers.push(await startServer(options.baseline));
    const baselineServer = servers[0];

    const mermaidPages = new Set(pages.filter((rel) => pageFeatures(options.baseline, rel).mermaid));
    for (const rel of mermaidPages) {
      report.soft_pages.push({
        page: rel,
        reason: 'mermaid static SVG (merman) vs baseline mermaid-cli SVG: accepted visual difference',
      });
    }

    const captureSet = async (server, siteDir, label) => {
      const dir = path.join(options.out, label);
      fs.mkdirSync(dir, { recursive: true });
      fs.mkdirSync(path.join(dir, 'diff'), { recursive: true });
      for (const rel of pages) {
        if (!fs.existsSync(path.join(siteDir, rel))) {
          report.captures[label].push({ page: rel, error: 'page missing in site' });
          continue;
        }
        for (const scheme of options.schemes) {
          const outFile = path.join(dir, `${pageSlug(rel)}__${scheme}.png`);
          const result = await capturePage(browser, server, rel, scheme, outFile, options);
          report.captures[label].push(result);
          log(options, `  captured ${label}/${rel} [${scheme}] ${result.width}x${result.height}${result.capped ? ' (capped)' : ''}`);
        }
      }
    };

    await captureSet(baselineServer, options.baseline, 'baseline');

    if (options.candidate) {
      const candidateServer = await startServer(options.candidate);
      servers.push(candidateServer);
      await captureSet(candidateServer, options.candidate, 'candidate');

      const baselineByName = new Map(report.captures.baseline
        .filter((entry) => entry.file).map((entry) => [`${entry.page}|${entry.scheme}`, entry]));
      const diffDir = path.join(options.out, 'diff');
      fs.mkdirSync(diffDir, { recursive: true });
      for (const candidateCapture of report.captures.candidate) {
        if (!candidateCapture.file) {
          report.comparisons.push({ page: candidateCapture.page, scheme: null, pass: false, error: candidateCapture.error });
          hardFailures += 1;
          continue;
        }
        const baselineCapture = baselineByName.get(`${candidateCapture.page}|${candidateCapture.scheme}`);
        if (!baselineCapture) {
          report.comparisons.push({ page: candidateCapture.page, scheme: candidateCapture.scheme, pass: false, error: 'no baseline capture' });
          hardFailures += 1;
          continue;
        }
        const diffFile = path.join(diffDir, `${pageSlug(candidateCapture.page)}__${candidateCapture.scheme}.png`);
        let result;
        try {
          result = await runPixdiff(baselineCapture.file, candidateCapture.file, diffFile, options.tol);
        } catch (error) {
          report.comparisons.push({ page: candidateCapture.page, scheme: candidateCapture.scheme, pass: false, error: error.message });
          hardFailures += 1;
          continue;
        }
        const soft = mermaidPages.has(candidateCapture.page);
        const pass = soft || result.differing_pct <= options.threshold;
        if (!pass) hardFailures += 1;
        report.comparisons.push({
          page: candidateCapture.page,
          scheme: candidateCapture.scheme,
          soft,
          pass,
          threshold_pct: options.threshold,
          diff_png: path.relative(options.out, diffFile),
          baseline_png: path.relative(options.out, baselineCapture.file),
          candidate_png: path.relative(options.out, candidateCapture.file),
          result,
        });
        log(options, `  ${pass ? (soft ? 'SOFT' : 'OK  ') : 'FAIL'} ${candidateCapture.page} [${candidateCapture.scheme}] `
          + `${result.differing_pct.toFixed(3)}% differ${soft ? ' (mermaid static, soft)' : ''}`);
      }
    }
  } catch (error) {
    console.error(`screenshots: ${error.stack || error.message}`);
    return 2;
  } finally {
    if (browser) await browser.close().catch(() => {});
    for (const server of servers) stopServer(server);
  }

  const durationSeconds = Math.round((Date.now() - started) / 100) / 10;
  report.summary = {
    duration_seconds: durationSeconds,
    captured_baseline: report.captures.baseline.filter((entry) => entry.file).length,
    captured_candidate: report.captures.candidate.filter((entry) => entry.file).length,
    comparisons: report.comparisons.length,
    hard_failures: hardFailures,
    mode: report.mode,
    result: hardFailures ? 'FAIL' : 'PASS',
  };
  const reportFile = path.join(options.out, 'report.json');
  fs.writeFileSync(reportFile, `${JSON.stringify(report, null, 2)}\n`);
  log(options, '');
  if (options.json) {
    console.log(JSON.stringify(report, null, 2));
  } else {
    if (options.candidate) {
      const failures = report.comparisons.filter((entry) => !entry.pass);
      const soft = report.comparisons.filter((entry) => entry.soft);
      log(options, `screenshots: ${report.summary.result} - ${report.comparisons.length} comparison(s), `
        + `${failures.length} hard failure(s), ${soft.length} soft (mermaid static)`);
      for (const failure of failures.slice(0, 10)) {
        log(options, `  ! ${failure.page} [${failure.scheme}] ${failure.result ? `${failure.result.differing_pct}% differ` : failure.error}`);
      }
    } else {
      log(options, `screenshots: captured ${report.summary.captured_baseline} baseline screenshot(s) (no candidate given)`);
    }
    log(options, `  report: ${reportFile}`);
    log(options, `  images: ${options.out}`);
  }
  return report.summary.result === 'PASS' ? 0 : 1;
}

main()
  .then((code) => { process.exitCode = code; })
  .catch((error) => {
    console.error(`screenshots: ${error.stack || error.message}`);
    process.exitCode = 2;
  });
