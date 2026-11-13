#!/usr/bin/env node
/**
 * Verify that generated Mermaid figures are truly static (no JavaScript needed).
 *
 * Since route A, blogen renders Mermaid fences to inline SVG at build time with
 * the pure-Rust `merman` renderer. Generated pages must therefore:
 *
 *   1. contain `<figure class="mermaid" data-mermaid-digest=…>` with an inline
 *      `<svg>` in both the light and the dark theme container;
 *   2. reference no Mermaid JavaScript (no `<script>` whose tag or body mentions
 *      mermaid) and link no Mermaid assets;
 *   3. ship no `mermaid*` files in the generated theme assets (`dist/`);
 *   4. render identically with JavaScript disabled: a Chromium capture with
 *      `setJavaScriptEnabled(false)` must show the same diagram SVGs and be
 *      pixel-identical to the JavaScript-enabled capture.
 *
 * Exit codes: 0 = all static/JS-free checks pass, 1 = a check failed,
 * 2 = usage/environment error.
 *
 * Usage:
 *   node scripts/verify/check_static_mermaid.mjs --site ../blog/public
 *   node scripts/verify/check_static_mermaid.mjs --site ../blog/public \
 *       --out scripts/verify/.cache/static-mermaid --json
 *
 * Options:
 *   --site DIR        generated site root to verify (default: ../blog/public)
 *   --out DIR         output dir for evidence screenshots/report
 *                     (default: scripts/verify/.cache/static-mermaid)
 *   --pages LIST      comma-separated page paths (default: detect pages with
 *                     static Mermaid figures)
 *   --schemes LIST    light,dark (default: light,dark)
 *   --blog-dir DIR    module root providing puppeteer (default: the legacy blog
 *                     checkout, falling back to .cache/legacy-deps)
 *   --max-height N    cap full-page screenshot height (default: 6000)
 *   --summary-out F   write a compact, deterministic evidence summary here
 *                     (used for docs/parity/static-mermaid-check.json)
 *   --json            print the machine-readable report to stdout
 */

import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '../..');
const DEFAULT_SITE = path.resolve(REPO_ROOT, '../blog/public');
const DEFAULT_OUT = path.join(SCRIPT_DIR, '.cache', 'static-mermaid');
const DEFAULT_BLOG = path.resolve(REPO_ROOT, '../blog');

const VIEWPORT = { width: 1100, height: 900 };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function parseArgs(argv) {
  const options = {
    site: DEFAULT_SITE,
    out: DEFAULT_OUT,
    pages: null,
    schemes: ['light', 'dark'],
    blogDir: DEFAULT_BLOG,
    blogDirExplicit: false,
    maxHeight: 6000,
    json: false,
    summaryOut: null,
  };
  const args = argv.slice();
  const takeValue = (index, name) => {
    if (args[index + 1] === undefined) throw new Error(`${name} needs a value`);
    return args[index + 1];
  };
  for (let i = 0; i < args.length; i += 1) {
    switch (args[i]) {
      case '--site': options.site = takeValue(i, '--site'); i += 1; break;
      case '--out': options.out = takeValue(i, '--out'); i += 1; break;
      case '--pages':
        options.pages = takeValue(i, '--pages').split(',').map((p) => p.trim()).filter(Boolean);
        i += 1;
        break;
      case '--schemes':
        options.schemes = takeValue(i, '--schemes').split(',').map((s) => s.trim()).filter(Boolean);
        i += 1;
        break;
      case '--blog-dir':
        options.blogDir = takeValue(i, '--blog-dir');
        options.blogDirExplicit = true;
        i += 1;
        break;
      case '--max-height': options.maxHeight = parseInt(takeValue(i, '--max-height'), 10); i += 1; break;
      case '--json': options.json = true; break;
      case '--summary-out': options.summaryOut = takeValue(i, '--summary-out'); i += 1; break;
      case '-h': case '--help':
        console.log('See the comment block at the top of check_static_mermaid.mjs for usage.');
        process.exit(0);
        break;
      default: throw new Error(`unknown argument: ${args[i]}`);
    }
  }
  options.site = path.resolve(options.site);
  options.out = path.resolve(options.out);
  if (!fs.existsSync(path.join(options.site, 'index.html'))) {
    throw new Error(`site has no index.html: ${options.site}`);
  }
  for (const scheme of options.schemes) {
    if (!['light', 'dark'].includes(scheme)) throw new Error(`unknown scheme: ${scheme}`);
  }
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

function loadPuppeteer(moduleRoot) {
  const requireFrom = createRequire(path.join(moduleRoot, 'node_modules', '__blogen_verify__.js'));
  return requireFrom('puppeteer');
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

// ---------------------------------------------------------------------------
// 1. Static markup audit
// ---------------------------------------------------------------------------
function matchAll(regex, text) {
  return [...text.matchAll(regex)];
}

function auditMarkup(siteDir) {
  const htmlFiles = walkFiles(siteDir).filter((rel) => rel.endsWith('.html') || rel.endsWith('.htm'));
  const pages = [];
  const errors = [];
  for (const rel of htmlFiles) {
    const html = readText(path.join(siteDir, rel));
    const figures = matchAll(/<figure class="mermaid"/g, html).length;
    if (figures === 0) continue;
    const page = {
      page: rel,
      figures,
      light_svgs: matchAll(/class="mermaid-light"[^>]*>\s*<svg\b/g, html).length,
      dark_svgs: matchAll(/class="mermaid-dark"[^>]*>\s*<svg\b/g, html).length,
      digests: matchAll(/data-mermaid-digest="([^"]+)"/g, html).map((m) => m[1]),
      mermaid_scripts: [],
      mermaid_asset_refs: [],
    };
    for (const match of matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi, html)) {
      const attrs = match[1] || '';
      const body = match[2] || '';
      if (/mermaid/i.test(attrs) || /mermaid/i.test(body)) {
        page.mermaid_scripts.push((attrs + body).replace(/\s+/g, ' ').trim().slice(0, 160));
      }
    }
    for (const match of matchAll(/(?:src|href)="([^"]*mermaid[^"]*)"/gi, html)) {
      page.mermaid_asset_refs.push(match[1]);
    }
    if (page.light_svgs < page.figures) {
      errors.push(`${rel}: ${page.figures} mermaid figure(s) but only ${page.light_svgs} light-theme inline <svg>`);
    }
    if (page.dark_svgs < page.figures) {
      errors.push(`${rel}: ${page.figures} mermaid figure(s) but only ${page.dark_svgs} dark-theme inline <svg>`);
    }
    if (page.mermaid_scripts.length) {
      errors.push(`${rel}: mermaid JavaScript is referenced by a <script> tag`);
    }
    if (page.mermaid_asset_refs.length) {
      errors.push(`${rel}: page links a mermaid asset: ${page.mermaid_asset_refs.join(', ')}`);
    }
    pages.push(page);
  }
  if (!pages.length) errors.push('no static Mermaid figures found in the site');
  return { pages, errors };
}

function auditAssets(siteDir) {
  const files = walkFiles(siteDir);
  const mermaidFiles = files.filter((rel) => /mermaid/i.test(path.basename(rel)));
  const errors = mermaidFiles.map((rel) => `generated site ships a mermaid asset: ${rel}`);
  return { files: files.length, mermaid_files: mermaidFiles, errors };
}

// ---------------------------------------------------------------------------
// 2. JS-disabled rendering check
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

function pageSlug(rel) {
  return rel.replace(/\.(html|htm)$/, '').replace(/[^A-Za-z0-9._-]+/g, '__') || 'index';
}

async function openPage(browser, server, options, rel, scheme, javaScriptEnabled) {
  const page = await browser.newPage();
  await page.setJavaScriptEnabled(javaScriptEnabled);
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
  await page.goto(pageUrl(server.baseUrl, rel), { waitUntil: 'networkidle0', timeout: 45000 });
  await page.evaluate(() => (document.fonts ? document.fonts.ready : Promise.resolve())).catch(() => {});
  await page.addStyleTag({
    content: '*,*::before,*::after{animation:none!important;transition:none!important;}'
      + 'html{scroll-behavior:auto!important;}*{caret-color:transparent!important;}',
  }).catch(() => {});
  await sleep(120);
  const info = await page.evaluate((darkScheme) => {
    const figures = document.querySelectorAll('figure.mermaid, .mermaid').length;
    const light = document.querySelectorAll('.mermaid-light svg').length;
    const dark = document.querySelectorAll('.mermaid-dark svg').length;
    const visible = darkScheme
      ? document.querySelector('.mermaid-dark svg')
      : document.querySelector('.mermaid-light svg');
    const rect = visible ? visible.getBoundingClientRect() : null;
    return {
      figures,
      light_svgs: light,
      dark_svgs: dark,
      visible_svg_width: rect ? Math.round(rect.width * 100) / 100 : 0,
      visible_svg_height: rect ? Math.round(rect.height * 100) / 100 : 0,
    };
  }, scheme === 'dark');
  const dims = await page.evaluate(() => ({
    width: Math.max(document.documentElement.scrollWidth, 1100),
    height: Math.max(
      document.body ? document.body.scrollHeight : 0,
      document.documentElement.scrollHeight,
    ),
  }));
  return { page, info, dims };
}

async function capture(page, dims, options, outFile) {
  const width = Math.min(Math.max(VIEWPORT.width, dims.width), 1400);
  const height = Math.max(VIEWPORT.height, Math.min(dims.height || VIEWPORT.height, options.maxHeight));
  await page.screenshot({
    path: outFile,
    clip: { x: 0, y: 0, width, height },
    captureBeyondViewport: true,
    optimizeForSpeed: false,
  });
  return { width, height };
}

function runPixdiff(a, b, diffOut, tol) {
  return new Promise((resolve, reject) => {
    const child = spawn('python3', [
      path.join(SCRIPT_DIR, 'pixdiff.py'),
      '--a', a, '--b', b, '--tol', String(tol), '--out-diff', diffOut,
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
      resolve(JSON.parse(stdout.trim().split('\n').pop()));
    });
  });
}

async function checkJsDisabled(browser, server, options, mermaidPages) {
  const checks = [];
  const errors = [];
  const outRoot = path.join(options.out, 'js-disabled');
  fs.mkdirSync(outRoot, { recursive: true });
  for (const rel of mermaidPages) {
    for (const scheme of options.schemes) {
      const slug = `${pageSlug(rel)}__${scheme}`;
      const onFile = path.join(outRoot, `${slug}__js-on.png`);
      const offFile = path.join(outRoot, `${slug}__js-off.png`);
      const diffFile = path.join(outRoot, `${slug}__diff.png`);
      const on = await openPage(browser, server, options, rel, scheme, true);
      try {
        const onShot = await capture(on.page, on.dims, options, onFile);
        const off = await openPage(browser, server, options, rel, scheme, false);
        try {
          const offShot = await capture(off.page, off.dims, options, offFile);
          const diff = await runPixdiff(onFile, offFile, diffFile, 12);
          const check = {
            page: rel,
            scheme,
            js_on: { ...on.info, ...onShot },
            js_off: { ...off.info, ...offShot },
            differing_pct: diff.differing_pct,
            differing_pixels: diff.differing_pixels,
            js_on_png: path.relative(options.out, onFile),
            js_off_png: path.relative(options.out, offFile),
            diff_png: path.relative(options.out, diffFile),
          };
          if (off.info.visible_svg_width <= 0 || off.info.visible_svg_height <= 0) {
            errors.push(`${rel} [${scheme}]: diagram SVG is not visible with JavaScript disabled`);
          }
          if (off.info.light_svgs !== on.info.light_svgs || off.info.dark_svgs !== on.info.dark_svgs) {
            errors.push(`${rel} [${scheme}]: SVG counts change when JavaScript is disabled`);
          }
          if (diff.differing_pct > 0.01) {
            errors.push(`${rel} [${scheme}]: JS-disabled rendering differs from JS-enabled `
              + `(${diff.differing_pct}% pixels)`);
          }
          checks.push(check);
        } finally {
          await off.page.close().catch(() => {});
        }
      } finally {
        await on.page.close().catch(() => {});
      }
    }
  }
  return { checks, errors };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`check_static_mermaid: ${error.message}`);
    return 2;
  }
  const puppeteerRoot = findPuppeteerRoot(options);
  if (!puppeteerRoot) {
    console.error('check_static_mermaid: puppeteer not found; run '
      + '`scripts/verify/make_baseline.sh --bootstrap-node-modules` or pass --blog-dir');
    return 2;
  }

  const report = {
    schema_version: 1,
    site: options.site,
    markup: auditMarkup(options.site),
    assets: auditAssets(options.site),
    js_disabled: { checks: [], errors: [] },
    summary: {},
  };
  const mermaidPages = options.pages
    || report.markup.pages.map((entry) => entry.page);
  if (!mermaidPages.length) {
    report.markup.errors.push('no Mermaid pages to verify');
  }

  let browser = null;
  let server = null;
  let chromiumVersion = null;
  fs.mkdirSync(options.out, { recursive: true });
  try {
    const requireFrom = createRequire(path.join(puppeteerRoot, 'node_modules', '__blogen_verify__.js'));
    const puppeteer = requireFrom('puppeteer');
    browser = await puppeteer.launch({
      headless: 'shell',
      args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
    });
    chromiumVersion = await browser.version();
    server = await startServer(options.site);
    report.js_disabled = await checkJsDisabled(browser, server, options, mermaidPages);
  } catch (error) {
    console.error(`check_static_mermaid: ${error.stack || error.message}`);
    return 2;
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (server && server.child.exitCode === null) server.child.kill('SIGTERM');
  }

  const errors = [...report.markup.errors, ...report.assets.errors, ...report.js_disabled.errors];
  report.summary = {
    mermaid_pages: report.markup.pages.length,
    figures: report.markup.pages.reduce((sum, entry) => sum + entry.figures, 0),
    inline_svgs: report.markup.pages.reduce((sum, entry) => sum + entry.light_svgs + entry.dark_svgs, 0),
    mermaid_scripts: report.markup.pages.reduce((sum, entry) => sum + entry.mermaid_scripts.length, 0),
    mermaid_assets: report.assets.mermaid_files.length,
    js_disabled_checks: report.js_disabled.checks.length,
    chromium: chromiumVersion,
    result: errors.length ? 'FAIL' : 'PASS',
    errors,
  };
  const reportFile = path.join(options.out, 'report.json');
  fs.writeFileSync(reportFile, `${JSON.stringify(report, null, 2)}\n`);

  if (options.summaryOut) {
    // Compact, deterministic evidence (no absolute paths / durations).
    const evidence = {
      schema_version: 1,
      note: 'generated by scripts/verify/check_static_mermaid.mjs',
      site_files: report.assets.files,
      mermaid_pages: report.markup.pages.map((page) => ({
        page: page.page,
        figures: page.figures,
        light_svgs: page.light_svgs,
        dark_svgs: page.dark_svgs,
        digests: page.digests,
        mermaid_scripts: page.mermaid_scripts.length,
        mermaid_asset_refs: page.mermaid_asset_refs.length,
      })),
      mermaid_assets: report.assets.mermaid_files,
      js_disabled: report.js_disabled.checks.map((check) => ({
        page: check.page,
        scheme: check.scheme,
        visible_svg_width: check.js_off.visible_svg_width,
        visible_svg_height: check.js_off.visible_svg_height,
        differing_pct_vs_js_enabled: check.differing_pct,
      })),
      chromium: report.summary.chromium || null,
      result: report.summary.result,
    };
    const summaryPath = path.resolve(options.summaryOut);
    fs.mkdirSync(path.dirname(summaryPath), { recursive: true });
    fs.writeFileSync(summaryPath, `${JSON.stringify(evidence, null, 2)}\n`);
    if (!options.json) console.log(`  evidence summary: ${summaryPath}`);
  }

  if (options.json) {
    console.log(JSON.stringify(report, null, 2));
  } else {
    for (const page of report.markup.pages) {
      console.log(`  static mermaid: ${page.page} - ${page.figures} figure(s), `
        + `${page.light_svgs} light + ${page.dark_svgs} dark inline SVG, no JS refs`);
    }
    for (const check of report.js_disabled.checks) {
      console.log(`  JS disabled: ${check.page} [${check.scheme}] `
        + `${check.js_off.visible_svg_width}x${check.js_off.visible_svg_height}px SVG visible, `
        + `${check.differing_pct.toFixed(3)}% pixels differ vs JS enabled`);
    }
    console.log(`check_static_mermaid: ${report.summary.result} - `
      + `${report.summary.mermaid_pages} page(s), ${report.summary.figures} figure(s), `
      + `${report.summary.js_disabled_checks} JS-disabled comparison(s)`);
    for (const error of errors) console.log(`  ! ${error}`);
    console.log(`  report: ${reportFile}`);
  }
  return errors.length ? 1 : 0;
}

main()
  .then((code) => { process.exitCode = code; })
  .catch((error) => {
    console.error(`check_static_mermaid: ${error.stack || error.message}`);
    process.exitCode = 2;
  });
