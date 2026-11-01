/* ══════════════ md.js：Markdown 解析与渲染管线 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  const escAttr = (s) => U.esc(s);

  /* ── 数学公式扩展（$…$ 行内 / $$…$$ 与 \[…\] 块级） ── */
  const inlineMath = {
    name: 'inlineMath',
    level: 'inline',
    start(src) { const i = src.search(/\$|\\\(/); return i < 0 ? undefined : i; },
    tokenizer(src) {
      const m = /^\\\((.+?)\\\)/.exec(src) || /^\$(?!\s)((?:\\.|[^\\$\n])+?)\$(?![\d\w])/.exec(src);
      if (m) return { type: 'inlineMath', raw: m[0], tex: m[1].trim() };
    },
    renderer(t) { return `<span class="math math-inline">${escAttr(t.tex)}</span>`; },
  };
  const blockMath = {
    name: 'blockMath',
    level: 'block',
    start(src) { const i = src.indexOf('$$'); const j = src.indexOf('\\['); const k = [i, j].filter((x) => x >= 0); return k.length ? Math.min(...k) : undefined; },
    tokenizer(src) {
      const m = /^\$\$([\s\S]+?)\$\$(?:\n+|$)/.exec(src) || /^\\\[([\s\S]+?)\\\](?:\n+|$)/.exec(src);
      if (m) return { type: 'blockMath', raw: m[0], tex: m[1].trim() };
    },
    renderer(t) { return `<div class="math math-block">${escAttr(t.tex)}</div>`; },
  };

  /* ── 标题锚点 ── */
  const slugCount = {};
  function slug(text) {
    let s = String(text).trim().toLowerCase()
      .replace(/[\s\u3000]+/g, '-')
      .replace(/[^\w\u4e00-\u9fff-]/g, '')
      .slice(0, 60) || 'section';
    if (slugCount[s] != null) { slugCount[s]++; s = `${s}-${slugCount[s]}`; } else slugCount[s] = 0;
    return s;
  }

  /* ── 渲染器 ── */
  const renderer = {
    heading(token) {
      const text = this.parser.parseInline(token.tokens);
      return `<h${token.depth} id="${escAttr(slug(token.text))}">${text}</h${token.depth}>\n`;
    },
    code(token) {
      const lang = (token.lang || '').trim().split(/\s+/)[0].toLowerCase();
      if (lang === 'mermaid') {
        // 代码放在元素文本中（textContent 往返无损；属性值会被 DOMPurify 截断/剥离）
        return `<pre class="mermaid-src" style="display:none">${escAttr(token.text)}</pre>`;
      }
      let html = '';
      try {
        if (lang && window.hljs && hljs.getLanguage(lang)) {
          html = hljs.highlight(token.text, { language: lang, ignoreIllegals: true }).value;
        } else if (window.hljs) {
          const auto = hljs.highlightAuto(token.text);
          html = auto.value;
        } else {
          html = escAttr(token.text);
        }
      } catch (e) { html = escAttr(token.text); }
      const label = lang ? `<span class="code-lang">${escAttr(lang)}</span>` : '';
      return `${label}<pre><code class="hljs language-${escAttr(lang || 'auto')}">${html}</code></pre>\n`;
    },
    link(token) {
      const text = this.parser.parseInline(token.tokens);
      const href = token.href || '';
      if (href.startsWith('#')) return `<a href="${escAttr(href)}">${text}</a>`;
      if (/^https?:/i.test(href)) return `<a href="${escAttr(href)}" data-external="1" title="${escAttr(token.title || '')}">${text}</a>`;
      return `<a href="${escAttr(href)}" data-md-link="1" title="${escAttr(token.title || '')}">${text}</a>`;
    },
    image(token) {
      return `<img src="${escAttr(token.href || '')}" alt="${escAttr(token.text || '')}" title="${escAttr(token.title || '')}" data-md-src="${escAttr(token.href || '')}" loading="lazy">`;
    },
    checkbox(token) {
      return `<input type="checkbox" ${token.checked ? 'checked' : ''} data-task="1">`;
    },
    listitem(token) {
      const inner = this.parser.parse(token.tokens, !!token.loose);
      if (token.task) return `<li class="task-list-item">${this.checkbox(token)}${inner}</li>\n`;
      return `<li>${inner}</li>\n`;
    },
    list(token) {
      const startAttr = token.ordered && token.start !== 1 && token.start !== '' ? ` start="${token.start}"` : '';
      const tag = token.ordered ? 'ol' : 'ul';
      const cls = !token.ordered && (token.items || []).some((i) => i.task) ? ' class="contains-task-list"' : '';
      return `<${tag}${cls}${startAttr}>\n${(token.items || []).map((i) => this.listitem(i)).join('')}</${tag}>\n`;
    },
    tablecell() { /* 走默认，无需定制 */ return false; },
  };

  let inited = false;
  function init() {
    if (inited) return;
    marked.use({ gfm: true, breaks: false, extensions: [inlineMath, blockMath], renderer });
    inited = true;
  }

  /* ── 图片相对路径解析缓存 ── */
  const imgCache = new Map();

  /* ── mermaid 懒加载 ── */
  let mermaidLoading = null;

  /** 统一的 mermaid 配置。两处 initialize 必须共用一份，否则改了一处另一处会覆盖回去。
   *
   * 关键：htmlLabels 必须为 false。
   * mermaid 的 HTML 标签是渲染在 <foreignObject> 里的普通 div/p，会**继承正文排版**：
   * 「仿 Word」这类主题给 .markdown-body p 设了 text-indent: 2em，于是标签文字被整体右移
   * 2em（16px 字号下就是 32px），而 foreignObject 的宽度仍是 mermaid 按无缩进算出来的值 →
   * 节点文字被裁掉半截、边上的「是/否」只剩一个空灰框。
   * 改成 SVG <text> 后标签不再受页面 CSS 影响，宽度测量与渲染同源，也不会被裁剪。
   * 换行仍然有效：源码里的 <br/> 会被拆成多个 <tspan>。 */
  function mermaidConfig() {
    const dark = document.documentElement.dataset.ui === 'dark';
    return {
      startOnLoad: false,
      securityLevel: 'strict',
      theme: dark ? 'dark' : 'default',
      fontFamily: 'Microsoft YaHei, Segoe UI, sans-serif',
      htmlLabels: false,
      flowchart: { htmlLabels: false, useMaxWidth: true },
      sequence: { useMaxWidth: true },
      gantt: { useMaxWidth: true },
      mindmap: { useMaxWidth: true },
    };
  }

  function ensureMermaid() {
    if (window.mermaid) return Promise.resolve();
    if (mermaidLoading) return mermaidLoading;
    mermaidLoading = new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = 'vendor/mermaid/mermaid.min.js';
      s.onload = () => {
        try {
          window.mermaid.initialize(mermaidConfig());
          resolve();
        } catch (e) { reject(e); }
      };
      s.onerror = () => reject(new Error('mermaid 加载失败'));
      document.head.appendChild(s);
    });
    return mermaidLoading;
  }

  /* ── mermaid 主题跟随界面明暗 ── */
  function refreshMermaidTheme() {
    if (!window.mermaid) return;
    try {
      window.mermaid.initialize(mermaidConfig());
    } catch (e) { /* 忽略 */ }
  }

  /* ── 后处理：公式 / mermaid / 图片 / 链接 ── */
  async function postprocess(rootEl, docDir) {
    const tasks = [];

    // 1) KaTeX
    if (window.katex) {
      rootEl.querySelectorAll('.math:not(.math-done)').forEach((el) => {
        const tex = el.textContent || '';
        if (!tex.trim()) return;
        el.classList.add('math-done');
        el.textContent = '';
        const displayMode = el.classList.contains('math-block');
        try {
          el.innerHTML = katex.renderToString(tex, {
            displayMode, throwOnError: false, strict: false, output: 'htmlAndMathml',
          });
          el.classList.add('math-done');
        } catch (e) {
          el.innerHTML = `<span class="math-error">${U.esc(String(e.message || e))}</span>`;
        }
      });
    }

    // 2) Mermaid
    const mermaids = Array.from(rootEl.querySelectorAll('pre.mermaid-src:not([data-done])'));
    if (mermaids.length) {
      tasks.push(ensureMermaid().then(async () => {
        for (const el of mermaids) {
          el.dataset.done = '1';
          const code = el.textContent || '';
          const box = document.createElement('div');
          box.className = 'mermaid-box';
          el.replaceWith(box);
          try {
            const { svg } = await window.mermaid.render('mmd-' + U.uid(), code);
            box.innerHTML = svg;
          } catch (e) {
            box.className = 'mermaid-error';
            box.textContent = 'Mermaid 渲染失败：' + String(e.message || e);
          }
        }
      }).catch((e) => {
        mermaids.forEach((el) => {
          el.className = 'mermaid-error';
          el.style.display = '';
          el.textContent = 'Mermaid 加载失败：' + e.message;
        });
      }));
    }

    // 3) 相对路径图片 → data URL
    if (docDir && MD.fsx.isApp) {
      rootEl.querySelectorAll('img[data-md-src]').forEach((img) => {
        const src = img.dataset.mdSrc;
        if (!src || /^(https?:|data:|file:)/i.test(src)) return;
        if (/^#/.test(src)) return;
        const full = U.joinPath(docDir, src.replace(/\\/g, '/'));
        // 必须把 promise 收进 tasks：否则导出/打印可能在图片转 dataURL 之前就取走 DOM，
        // 导出的文件里留下失效的相对路径。
        tasks.push((async () => {
          try {
            let url;
            if (imgCache.has(full)) url = imgCache.get(full);
            else { url = await MD.fsx.readImageDataURL(full); imgCache.set(full, url); }
            if (img.isConnected) img.src = url;
          } catch (e) { /* 图片缺失，保留原样 */ }
        })());
      });
    }

    await Promise.all(tasks);
  }

  /* ── 主入口：源码 → DOM ── */
  async function render(source, targetEl, docDir) {
    init();
    slugCountReset();
    let html = '';
    try {
      html = marked.parse(source || '', { async: false });
    } catch (e) {
      html = `<p class="math-error">Markdown 解析出错：${U.esc(String(e.message || e))}</p>`;
    }
    html = DOMPurify.sanitize(html, {
      ADD_ATTR: ['data-tex', 'data-src', 'data-md-src', 'data-md-link', 'data-external', 'data-task',
        'target', 'id', 'class', 'align', 'start', 'type', 'checked', 'loading', 'colspan', 'rowspan', 'title'],
      ADD_TAGS: ['details', 'summary'],
      ALLOW_UNKNOWN_PROTOCOLS: false,
    });
    targetEl.innerHTML = html || '<p style="color:var(--doc-muted)">（空文档）</p>';
    await postprocess(targetEl, docDir);
    return targetEl;
  }

  function slugCountReset() { for (const k of Object.keys(slugCount)) delete slugCount[k]; }

  /* ── 大纲：直接扫描源码行 ── */
  function parseOutline(source) {
    const out = [];
    let inCode = false;
    source.split('\n').forEach((line, i) => {
      const fence = /^\s*(```|~~~)/.exec(line);
      if (fence) { inCode = !inCode; return; }
      if (inCode) return;
      const m = /^(#{1,6})\s+(.+?)\s*#*\s*$/.exec(line);
      if (m) {
        const text = m[2].replace(/[*`~]/g, '').trim();
        out.push({ level: m[1].length, text, line: i });
      }
    });
    return out;
  }

  MD.md = { render, parseOutline, postprocess, imgCache, refreshMermaidTheme };
})();
