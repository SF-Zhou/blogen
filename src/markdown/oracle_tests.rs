//! Golden unit tests for the markdown pipeline.
//!
//! Every `EXPECTED_*` constant below was produced by the reference
//! pipeline (`scripts/reference/marked.js`: markdown-it 14.3 + the exact plugin
//! versions used by the blog) from the `SOURCE_*` constant of the same test.
//! They pin down the byte-level markup of each feature: heading anchors and
//! slug de-duplication, the table of contents, implicit figures, the four
//! texmath math wrappers, Prism fences (aliases/unknown/plain), linkify, tables,
//! lists, escaping and entities.
//!
//! Regenerate with `node scripts/reference/render.mjs` snippets (or the
//! `scripts/reference/snippets.mjs` list); the KaTeX output contains
//! zero-width spaces, hence the `invisible_characters` allowance.
#![allow(clippy::invisible_characters)]

use super::{MarkdownOptions, MarkdownRenderer};

fn render(markdown: &str) -> String {
    MarkdownRenderer::new(MarkdownOptions::default())
        .render(markdown)
        .expect("markdown render")
        .html
}

const SOURCE_HEADING_SLUGS: &str = r"# Hello   World

## Hello World

### Hello World

#### Hello World
";

const EXPECTED_HEADING_SLUGS: &str = r##"<h1 id="hello-world" tabindex="-1"><a href="#hello-world">Hello   World</a></h1>
<h2 id="hello-world-1" tabindex="-1"><a href="#hello-world-1">Hello World</a></h2>
<h3 id="hello-world-2" tabindex="-1"><a href="#hello-world-2">Hello World</a></h3>
<h4 id="hello-world-3" tabindex="-1"><a href="#hello-world-3">Hello World</a></h4>
"##;

#[test]
fn heading_slugs() {
    assert_eq!(render(SOURCE_HEADING_SLUGS), EXPECTED_HEADING_SLUGS);
}

const SOURCE_HEADING_UNICODE: &str = r"# RuaPC：安全的 RDMA 零拷贝

## 1. Flynn 分类法
";

const EXPECTED_HEADING_UNICODE: &str = r##"<h1 id="ruapc：安全的-rdma-零拷贝" tabindex="-1"><a href="#ruapc%EF%BC%9A%E5%AE%89%E5%85%A8%E7%9A%84-rdma-%E9%9B%B6%E6%8B%B7%E8%B4%9D">RuaPC：安全的 RDMA 零拷贝</a></h1>
<h2 id="1.-flynn-分类法" tabindex="-1"><a href="#1.-flynn-%E5%88%86%E7%B1%BB%E6%B3%95">1. Flynn 分类法</a></h2>
"##;

#[test]
fn heading_unicode() {
    assert_eq!(render(SOURCE_HEADING_UNICODE), EXPECTED_HEADING_UNICODE);
}

const SOURCE_HEADING_MARKUP: &str = r"# A *b* and `c_x` and [d](https://e.f)
";

const EXPECTED_HEADING_MARKUP: &str = r##"<h1 id="a-b-and-c_x-and-d" tabindex="-1"><a href="#a-b-and-c_x-and-d">A <em>b</em> and <code>c_x</code> and <a href="https://e.f">d</a></a></h1>
"##;

#[test]
fn heading_markup() {
    assert_eq!(render(SOURCE_HEADING_MARKUP), EXPECTED_HEADING_MARKUP);
}

const SOURCE_TOC: &str = r"Intro.

[TOC]

# One

text

### Three

text

## Two

text
";

const EXPECTED_TOC: &str = r##"<p>Intro.</p>
<div class="table-of-contents"><ul><li><a href="#one">One</a><ul><li><ul><li><a href="#three">Three</a></li></ul></li><li><a href="#two">Two</a></li></ul></li></ul></div>
<h1 id="one" tabindex="-1"><a href="#one">One</a></h1>
<p>text</p>
<h3 id="three" tabindex="-1"><a href="#three">Three</a></h3>
<p>text</p>
<h2 id="two" tabindex="-1"><a href="#two">Two</a></h2>
<p>text</p>
"##;

#[test]
fn toc() {
    assert_eq!(render(SOURCE_TOC), EXPECTED_TOC);
}

const SOURCE_FIGURE_IMAGE: &str = r#"![Caption *emph*][img]

[img]: /images/photo.png "Title"
"#;

const EXPECTED_FIGURE_IMAGE: &str = r#"<figure tabindex="1"><a href="/images/photo.png"><img src="/images/photo.png" alt="" title="Title"></a><figcaption>Caption <em>emph</em></figcaption></figure>
"#;

#[test]
fn figure_image() {
    assert_eq!(render(SOURCE_FIGURE_IMAGE), EXPECTED_FIGURE_IMAGE);
}

const SOURCE_FIGURE_LINKED: &str = r"[![Alt text](/images/a.png)](/target.html)

![](/images/b.png)
";

const EXPECTED_FIGURE_LINKED: &str = r#"<figure tabindex="1"><a href="/target.html"><img src="/images/a.png" alt=""></a><figcaption>Alt text</figcaption></figure>
<figure tabindex="2"><a href="/images/b.png"><img src="/images/b.png" alt=""></a></figure>
"#;

#[test]
fn figure_linked() {
    assert_eq!(render(SOURCE_FIGURE_LINKED), EXPECTED_FIGURE_LINKED);
}

const SOURCE_MATH_INLINE: &str = r"Euler: $e^{ix} = \cos x + i\sin x$ done.
";

const EXPECTED_MATH_INLINE: &str = r#"<p>Euler: <eq><span class="katex"><span class="katex-mathml"><math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><msup><mi>e</mi><mrow><mi>i</mi><mi>x</mi></mrow></msup><mo>=</mo><mi>cos</mi><mo>⁡</mo><mi>x</mi><mo>+</mo><mi>i</mi><mi>sin</mi><mo>⁡</mo><mi>x</mi></mrow><annotation encoding="application/x-tex">e^{ix} = \cos x + i\sin x</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="katex-base"><span class="katex-strut" style="height:0.8247em;"></span><span class="mord"><span class="mord mathnormal">e</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8247em;"><span style="top:-3.063em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight"><span class="mord mathnormal mtight">i</span><span class="mord mathnormal mtight">x</span></span></span></span></span></span></span></span></span><span class="mspace" style="margin-right:0.2778em;"></span><span class="mrel">=</span><span class="mspace" style="margin-right:0.2778em;"></span></span><span class="katex-base"><span class="katex-strut" style="height:0.6667em;vertical-align:-0.0833em;"></span><span class="mop">cos</span><span class="mspace" style="margin-right:0.1667em;"></span><span class="mord mathnormal">x</span><span class="mspace" style="margin-right:0.2222em;"></span><span class="mbin">+</span><span class="mspace" style="margin-right:0.2222em;"></span></span><span class="katex-base"><span class="katex-strut" style="height:0.6679em;"></span><span class="mord mathnormal">i</span><span class="mspace" style="margin-right:0.1667em;"></span><span class="mop">sin</span><span class="mspace" style="margin-right:0.1667em;"></span><span class="mord mathnormal">x</span></span></span></span></eq> done.</p>
"#;

#[test]
fn math_inline() {
    assert_eq!(render(SOURCE_MATH_INLINE), EXPECTED_MATH_INLINE);
}

const SOURCE_MATH_INLINE_DOUBLE: &str = r"Inline $$a^2 + b^2 = c^2$$ display.
";

const EXPECTED_MATH_INLINE_DOUBLE: &str = r#"<p>Inline <section><eqn><span class="katex-display"><span class="katex"><span class="katex-mathml"><math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow><msup><mi>a</mi><mn>2</mn></msup><mo>+</mo><msup><mi>b</mi><mn>2</mn></msup><mo>=</mo><msup><mi>c</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">a^2 + b^2 = c^2</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="katex-base"><span class="katex-strut" style="height:0.9474em;vertical-align:-0.0833em;"></span><span class="mord"><span class="mord mathnormal">a</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8641em;"><span style="top:-3.113em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">2</span></span></span></span></span></span></span></span><span class="mspace" style="margin-right:0.2222em;"></span><span class="mbin">+</span><span class="mspace" style="margin-right:0.2222em;"></span></span><span class="katex-base"><span class="katex-strut" style="height:0.8641em;"></span><span class="mord"><span class="mord mathnormal">b</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8641em;"><span style="top:-3.113em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">2</span></span></span></span></span></span></span></span><span class="mspace" style="margin-right:0.2778em;"></span><span class="mrel">=</span><span class="mspace" style="margin-right:0.2778em;"></span></span><span class="katex-base"><span class="katex-strut" style="height:0.8641em;"></span><span class="mord"><span class="mord mathnormal">c</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8641em;"><span style="top:-3.113em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">2</span></span></span></span></span></span></span></span></span></span></span></span></eqn></section> display.</p>
"#;

#[test]
fn math_inline_double() {
    assert_eq!(
        render(SOURCE_MATH_INLINE_DOUBLE),
        EXPECTED_MATH_INLINE_DOUBLE
    );
}

const SOURCE_MATH_BLOCK: &str = r"$$
\int_0^1 x^2 dx
$$
";

const EXPECTED_MATH_BLOCK: &str = r#"<section><eqn><span class="katex-display"><span class="katex"><span class="katex-mathml"><math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow><msubsup><mo>∫</mo><mn>0</mn><mn>1</mn></msubsup><msup><mi>x</mi><mn>2</mn></msup><mi>d</mi><mi>x</mi></mrow><annotation encoding="application/x-tex">
\int_0^1 x^2 dx
</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="katex-base"><span class="katex-strut" style="height:2.476em;vertical-align:-0.9119em;"></span><span class="mop"><span class="mop op-symbol large-op" style="margin-right:0.4445em;position:relative;top:-0.0011em;">∫</span><span class="msupsub"><span class="vlist-t vlist-t2"><span class="vlist-r"><span class="vlist" style="height:1.564em;"><span style="top:-1.7881em;margin-left:-0.4445em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">0</span></span></span><span style="top:-3.8129em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">1</span></span></span></span><span class="vlist-s">​</span></span><span class="vlist-r"><span class="vlist" style="height:0.9119em;"><span></span></span></span></span></span></span><span class="mspace" style="margin-right:0.1667em;"></span><span class="mord"><span class="mord mathnormal">x</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8641em;"><span style="top:-3.113em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">2</span></span></span></span></span></span></span></span><span class="mord mathnormal">d</span><span class="mord mathnormal">x</span></span></span></span></span></eqn></section>"#;

#[test]
fn math_block() {
    assert_eq!(render(SOURCE_MATH_BLOCK), EXPECTED_MATH_BLOCK);
}

const SOURCE_MATH_EQNO: &str = r"$$
E = mc^2
$$ (42)
";

const EXPECTED_MATH_EQNO: &str = r#"<section class="eqno"><eqn><span class="katex-display"><span class="katex"><span class="katex-mathml"><math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">
E = mc^2
</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="katex-base"><span class="katex-strut" style="height:0.6833em;"></span><span class="mord mathnormal" style="margin-right:0.0576em;">E</span><span class="mspace" style="margin-right:0.2778em;"></span><span class="mrel">=</span><span class="mspace" style="margin-right:0.2778em;"></span></span><span class="katex-base"><span class="katex-strut" style="height:0.8641em;"></span><span class="mord mathnormal">m</span><span class="mord"><span class="mord mathnormal">c</span><span class="msupsub"><span class="vlist-t"><span class="vlist-r"><span class="vlist" style="height:0.8641em;"><span style="top:-3.113em;margin-right:0.05em;"><span class="pstrut" style="height:2.7em;"></span><span class="katex-sizing reset-size6 size3 mtight"><span class="mord mtight">2</span></span></span></span></span></span></span></span></span></span></span></span></eqn><span>(42)</span></section>"#;

#[test]
fn math_eqno() {
    assert_eq!(render(SOURCE_MATH_EQNO), EXPECTED_MATH_EQNO);
}

const SOURCE_CODE_RUST: &str = r"```rust
let x = 1;
/// doc
```
";

const EXPECTED_CODE_RUST: &str = r#"<pre class="language-rust"><code class="language-rust"><span class="token keyword">let</span> x <span class="token operator">=</span> <span class="token number">1</span><span class="token punctuation">;</span>
<span class="token comment">/// doc</span>
</code></pre>
"#;

#[test]
fn code_rust() {
    assert_eq!(render(SOURCE_CODE_RUST), EXPECTED_CODE_RUST);
}

const SOURCE_CODE_CPP_ALIAS: &str = r"```c++
int main() { return 0; }
```
";

const EXPECTED_CODE_CPP_ALIAS: &str = r#"<pre class="language-cpp"><code class="language-cpp"><span class="token keyword">int</span> <span class="token function">main</span><span class="token punctuation">(</span><span class="token punctuation">)</span> <span class="token punctuation">{</span> <span class="token keyword">return</span> <span class="token number">0</span><span class="token punctuation">;</span> <span class="token punctuation">}</span>
</code></pre>
"#;

#[test]
fn code_cpp_alias() {
    assert_eq!(render(SOURCE_CODE_CPP_ALIAS), EXPECTED_CODE_CPP_ALIAS);
}

const SOURCE_CODE_EMPTY_INFO: &str = r"```
plain <text> & stuff
```
";

const EXPECTED_CODE_EMPTY_INFO: &str = r#"<pre class="language-markup"><code class="language-markup">plain <span class="token tag"><span class="token tag"><span class="token punctuation">&lt;</span>text</span><span class="token punctuation">></span></span> &amp; stuff
</code></pre>
"#;

#[test]
fn code_empty_info() {
    assert_eq!(render(SOURCE_CODE_EMPTY_INFO), EXPECTED_CODE_EMPTY_INFO);
}

const SOURCE_CODE_UNKNOWN: &str = r"```sql
SELECT * FROM t WHERE a = 1;
```
";

const EXPECTED_CODE_UNKNOWN: &str = r#"<pre class="language-sql"><code class="language-sql">SELECT * FROM t WHERE a = 1;
</code></pre>
"#;

#[test]
fn code_unknown() {
    assert_eq!(render(SOURCE_CODE_UNKNOWN), EXPECTED_CODE_UNKNOWN);
}

const SOURCE_CODE_UPPER_ALIAS: &str = r"```Rust
fn main() {}
```
";

const EXPECTED_CODE_UPPER_ALIAS: &str = r#"<pre class="language-rust"><code class="language-rust"><span class="token keyword">fn</span> <span class="token function-definition function">main</span><span class="token punctuation">(</span><span class="token punctuation">)</span> <span class="token punctuation">{</span><span class="token punctuation">}</span>
</code></pre>
"#;

#[test]
fn code_upper_alias() {
    assert_eq!(render(SOURCE_CODE_UPPER_ALIAS), EXPECTED_CODE_UPPER_ALIAS);
}

const SOURCE_CODE_YML_ASM_SHELL: &str = r"```yml
a: 1
```

```asm
mov eax, 1
```

```shell
echo hi
```
";

const EXPECTED_CODE_YML_ASM_SHELL: &str = r#"<pre class="language-yaml"><code class="language-yaml"><span class="token key atrule">a</span><span class="token punctuation">:</span> <span class="token number">1</span>
</code></pre>
<pre class="language-nasm"><code class="language-nasm">mov <span class="token register variable">eax</span>, <span class="token number">1</span>
</code></pre>
<pre class="language-shell"><code class="language-shell"><span class="token builtin class-name">echo</span> hi
</code></pre>
"#;

#[test]
fn code_yml_asm_shell() {
    assert_eq!(
        render(SOURCE_CODE_YML_ASM_SHELL),
        EXPECTED_CODE_YML_ASM_SHELL
    );
}

const SOURCE_CODE_INFO_EXTRA: &str = r"```cpp title=foo
int a;
```
";

const EXPECTED_CODE_INFO_EXTRA: &str = r#"<pre class="language-cpp"><code class="language-cpp"><span class="token keyword">int</span> a<span class="token punctuation">;</span>
</code></pre>
"#;

#[test]
fn code_info_extra() {
    assert_eq!(render(SOURCE_CODE_INFO_EXTRA), EXPECTED_CODE_INFO_EXTRA);
}

const SOURCE_INDENT_CODE: &str = r"    code <x> & y
";

const EXPECTED_INDENT_CODE: &str = r"<pre><code>code &lt;x&gt; &amp; y
</code></pre>
";

#[test]
fn indent_code() {
    assert_eq!(render(SOURCE_INDENT_CODE), EXPECTED_INDENT_CODE);
}

const SOURCE_LINKS_TITLES: &str = r#"[text](https://example.com "the title") and <https://a.b/c> and <a@b.com>
"#;

const EXPECTED_LINKS_TITLES: &str = r#"<p><a href="https://example.com" title="the title">text</a> and <a href="https://a.b/c">https://a.b/c</a> and <a href="mailto:a@b.com">a@b.com</a></p>
"#;

#[test]
fn links_titles() {
    assert_eq!(render(SOURCE_LINKS_TITLES), EXPECTED_LINKS_TITLES);
}

const SOURCE_LINKS_REFERENCE: &str = r#"[ref][id] and [id][] and [id]

[id]: https://example.org/x "T"
"#;

const EXPECTED_LINKS_REFERENCE: &str = r#"<p><a href="https://example.org/x" title="T">ref</a> and <a href="https://example.org/x" title="T">id</a> and <a href="https://example.org/x" title="T">id</a></p>
"#;

#[test]
fn links_reference() {
    assert_eq!(render(SOURCE_LINKS_REFERENCE), EXPECTED_LINKS_REFERENCE);
}

const SOURCE_LINKIFY_BARE: &str = r"See https://github.com/a/b_c and www.example.com and foo@bar.com now.
";

const EXPECTED_LINKIFY_BARE: &str = r#"<p>See <a href="https://github.com/a/b_c">https://github.com/a/b_c</a> and <a href="http://www.example.com">www.example.com</a> and <a href="mailto:foo@bar.com">foo@bar.com</a> now.</p>
"#;

#[test]
fn linkify_bare() {
    assert_eq!(render(SOURCE_LINKIFY_BARE), EXPECTED_LINKIFY_BARE);
}

const SOURCE_LINKIFY_PUNCT: &str = r"Go to https://example.com/x. Then (https://example.com/(y)) and https://example.com/z, done
";

const EXPECTED_LINKIFY_PUNCT: &str = r#"<p>Go to <a href="https://example.com/x">https://example.com/x</a>. Then (<a href="https://example.com/(y)">https://example.com/(y)</a>) and <a href="https://example.com/z">https://example.com/z</a>, done</p>
"#;

#[test]
fn linkify_punct() {
    assert_eq!(render(SOURCE_LINKIFY_PUNCT), EXPECTED_LINKIFY_PUNCT);
}

const SOURCE_IMAGES: &str = r#"![alt](/i.png "t") and ![](/j.png)
"#;

const EXPECTED_IMAGES: &str = r#"<p><img src="/i.png" alt="alt" title="t"> and <img src="/j.png" alt=""></p>
"#;

#[test]
fn images() {
    assert_eq!(render(SOURCE_IMAGES), EXPECTED_IMAGES);
}

const SOURCE_TABLES: &str = r"| left | center | right | none |
|:--|:-:|--:|---|
| a | b | c | d |
| e | f | g | h |
";

const EXPECTED_TABLES: &str = r#"<table>
<thead>
<tr>
<th style="text-align:left">left</th>
<th style="text-align:center">center</th>
<th style="text-align:right">right</th>
<th>none</th>
</tr>
</thead>
<tbody>
<tr>
<td style="text-align:left">a</td>
<td style="text-align:center">b</td>
<td style="text-align:right">c</td>
<td>d</td>
</tr>
<tr>
<td style="text-align:left">e</td>
<td style="text-align:center">f</td>
<td style="text-align:right">g</td>
<td>h</td>
</tr>
</tbody>
</table>
"#;

#[test]
fn tables() {
    assert_eq!(render(SOURCE_TABLES), EXPECTED_TABLES);
}

const SOURCE_LISTS_TIGHT: &str = r"- a
- b
  - b1
  - b2
- c
";

const EXPECTED_LISTS_TIGHT: &str = r"<ul>
<li>a</li>
<li>b
<ul>
<li>b1</li>
<li>b2</li>
</ul>
</li>
<li>c</li>
</ul>
";

#[test]
fn lists_tight() {
    assert_eq!(render(SOURCE_LISTS_TIGHT), EXPECTED_LISTS_TIGHT);
}

const SOURCE_LISTS_LOOSE: &str = r"- a

- b

  paragraph
";

const EXPECTED_LISTS_LOOSE: &str = r"<ul>
<li>
<p>a</p>
</li>
<li>
<p>b</p>
<p>paragraph</p>
</li>
</ul>
";

#[test]
fn lists_loose() {
    assert_eq!(render(SOURCE_LISTS_LOOSE), EXPECTED_LISTS_LOOSE);
}

const SOURCE_LISTS_ORDERED: &str = r"3. a
4. b
";

const EXPECTED_LISTS_ORDERED: &str = r#"<ol start="3">
<li>a</li>
<li>b</li>
</ol>
"#;

#[test]
fn lists_ordered() {
    assert_eq!(render(SOURCE_LISTS_ORDERED), EXPECTED_LISTS_ORDERED);
}

const SOURCE_BLOCKQUOTE_LIST: &str = r"> - a
> - b
>
> text
";

const EXPECTED_BLOCKQUOTE_LIST: &str = r"<blockquote>
<ul>
<li>a</li>
<li>b</li>
</ul>
<p>text</p>
</blockquote>
";

#[test]
fn blockquote_list() {
    assert_eq!(render(SOURCE_BLOCKQUOTE_LIST), EXPECTED_BLOCKQUOTE_LIST);
}

const SOURCE_TASKLIST_LITERAL: &str = r"- [x] done
- [ ] todo
";

const EXPECTED_TASKLIST_LITERAL: &str = r"<ul>
<li>[x] done</li>
<li>[ ] todo</li>
</ul>
";

#[test]
fn tasklist_literal() {
    assert_eq!(render(SOURCE_TASKLIST_LITERAL), EXPECTED_TASKLIST_LITERAL);
}

const SOURCE_MISC_INLINE: &str = r"**strong** *em* ~~del~~ `code` and  \*literal\* &amp; entity

line  
break

---
";

const EXPECTED_MISC_INLINE: &str = r"<p><strong>strong</strong> <em>em</em> <s>del</s> <code>code</code> and  *literal* &amp; entity</p>
<p>line<br>
break</p>
<hr>
";

#[test]
fn misc_inline() {
    assert_eq!(render(SOURCE_MISC_INLINE), EXPECTED_MISC_INLINE);
}

const SOURCE_HTML_ESCAPED: &str = r#"<div class="x">
**not bold**
</div>

text <span>inline</span> end
"#;

const EXPECTED_HTML_ESCAPED: &str = r"<p>&lt;div class=&quot;x&quot;&gt;
<strong>not bold</strong>
&lt;/div&gt;</p>
<p>text &lt;span&gt;inline&lt;/span&gt; end</p>
";

#[test]
fn html_escaped() {
    assert_eq!(render(SOURCE_HTML_ESCAPED), EXPECTED_HTML_ESCAPED);
}

const SOURCE_ESCAPES: &str = r"\# not heading and \[not link\]
";

const EXPECTED_ESCAPES: &str = r"<p># not heading and [not link]</p>
";

#[test]
fn escapes() {
    assert_eq!(render(SOURCE_ESCAPES), EXPECTED_ESCAPES);
}

const SOURCE_ENTITY: &str = r"&copy; &#65; &amp;lt;
";

const EXPECTED_ENTITY: &str = r"<p>© A &amp;lt;</p>
";

#[test]
fn entity() {
    assert_eq!(render(SOURCE_ENTITY), EXPECTED_ENTITY);
}

const SOURCE_EMPH_EDGE: &str = r"a_b_c and _x_ and __y__ and ***z***
";

const EXPECTED_EMPH_EDGE: &str = r"<p>a_b_c and <em>x</em> and <strong>y</strong> and <em><strong>z</strong></em></p>
";

#[test]
fn emph_edge() {
    assert_eq!(render(SOURCE_EMPH_EDGE), EXPECTED_EMPH_EDGE);
}
