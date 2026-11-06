/* ══════════════ actions.js：文档打开 / 保存 / 导出 / 图片 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  const state = MD.state;

  function markDirty(on) {
    state.dirty = on !== false;
    const el = U.$('#st-save');
    if (state.dirty) { el.textContent = '● 未保存'; el.classList.add('dirty'); }
    else { el.textContent = '已保存'; el.classList.remove('dirty'); }
    MD.app.updateWindowTitle();
  }
  MD.bus.on('doc:changed', () => markDirty(true));
  MD.bus.on('doc:saved', () => markDirty(false));

  /** 打开/新建文档意味着要动笔：如果当前停在阅读模式（编辑区被隐藏），先切回编辑视图 */
  function ensureEditable() {
    if (document.documentElement.dataset.view === 'read' && MD.app && MD.app.setView) MD.app.setView('split');
  }

  /* ══════════ 新建 ══════════ */
  async function newFile(inDir) {
    if (state.dirty && !confirm('当前文档未保存，确定放弃并新建吗？')) return;
    ensureEditable();
    state.currentFile = null;
    state.currentDir = inDir || null;
    MD.editor.value = MD.defaults.newDocTemplate || '# 未命名文档\n\n';
    MD.editor.focus();
    MD.bus.emit('doc:opened', null);
    markDirty(false);
    MD.app.renderPreview();
  }

  /* ══════════ 打开 ══════════ */
  async function openFileDialog() {
    const p = await MD.fsx.pickFile([
      { name: 'Markdown', extensions: ['md', 'markdown'] },
      { name: '所有文件', extensions: ['*'] },
    ]);
    if (p) openFile(p);
  }
  async function openFolderDialog() {
    const p = await MD.fsx.pickFolder();
    if (!p) return;
    MD.panels.setFolder(p);
    MD.settings.lastFolder = p;
    MD.saveSettings();
    MD.toast('已打开文件夹');
  }
  /* ══════════ 导入：PDF / Word / Excel / PPT / HTML → Markdown ══════════ */
  const CONVERT_EXTS = ['pdf', 'docx', 'doc', 'xlsx', 'xlsm', 'pptx', 'html', 'htm', 'rtf', 'csv', 'tsv', 'txt'];
  const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'];

  function isConvertible(p) {
    const e = U.ext(p);
    return CONVERT_EXTS.includes(e) || ['md', 'markdown'].includes(e);
  }

  /** 把任意支持的文档转成 .md（输出到同目录同名 .md）并打开 */
  async function importFile(path) {
    if (!MD.fsx.isApp) {
      MD.toast('浏览器版本不支持格式转换，请使用桌面版');
      return;
    }
    let out = String(path).replace(/\.[^.\\/]+$/, '') + '.md';
    // 不静默覆盖同目录已有的同名笔记，自动加 -1 / -2 后缀
    try {
      if (await MD.fsx.exists(out)) {
        const base = out.replace(/\.md$/i, '');
        let i = 1;
        while (await MD.fsx.exists(`${base}-${i}.md`)) i++;
        out = `${base}-${i}.md`;
      }
    } catch (e) { /* 检查失败就按原路径，交给转换命令报错 */ }
    MD.toast('正在转换 ' + U.baseName(path) + ' …', 60000);
    try {
      const res = await MD.fsx.convertToMd(path, out, { images: true, overwrite: true });
      // 后端返回 { path, warnings }；兼容只返回路径字符串的旧后端
      const outPath = res && typeof res === 'object' ? res.path : res;
      const warns = res && Array.isArray(res.warnings) ? res.warnings : [];
      if (warns.length) {
        // 转换完成但结果需要核对（缺字回退、矢量图未转图片、解析中断等）：提示停留更久，完整列表进控制台
        console.warn('转换提示：', warns);
        const more = warns.length > 1 ? ` 等 ${warns.length} 条` : '';
        MD.toast(`已转换：${U.baseName(outPath)}（提示：${warns[0]}${more}）`, 9000);
      } else {
        MD.toast('已转换：' + U.baseName(outPath));
      }
      await openFile(outPath);
    } catch (e) {
      MD.toast('转换失败：' + (e && e.message ? e.message : e));
    }
  }

  async function importAsMarkdown() {
    const p = await MD.fsx.pickFile([
      { name: '可转换文档', extensions: CONVERT_EXTS },
      { name: '所有文件', extensions: ['*'] },
    ]);
    if (p) await importFile(p);
  }

  async function openFile(path) {
    if (state.dirty && !confirm('当前文档未保存，确定放弃并打开新文件吗？')) return;
    ensureEditable();
    try {
      const text = await MD.fsx.readFile(path);
      state.currentFile = path;
      state.currentDir = U.dirName(path);
      MD.editor.value = text;
      MD.panels.pushRecent(path);
      if (!MD.panels.currentFolder && MD.fsx.isApp) {
        const dir = U.dirName(path);
        MD.panels.setFolder(dir);
        // 自动设置文件夹时一并记住，供下次启动「恢复上次文件夹」
        MD.settings.lastFolder = dir;
        MD.saveSettings();
      }
      MD.bus.emit('doc:opened', path);
      markDirty(false);
      MD.app.renderPreview();
      MD.editor.focus();
    } catch (e) {
      MD.toast('打开失败：' + e.message);
    }
  }

  /* ══════════ 保存 ══════════ */
  /** 写盘期间文档是否没被再次改动：用 changeGeneration 快照 + isClean 比对 */
  function isCleanSince(gen) {
    const cm = MD.editor.cm;
    return !gen || !!(cm && cm.isClean && cm.isClean(gen));
  }
  async function save() {
    if (!state.currentFile) return saveAs();
    const gen = MD.editor.cm && MD.editor.cm.changeGeneration ? MD.editor.cm.changeGeneration() : null;
    try {
      await MD.fsx.writeFile(state.currentFile, MD.editor.value);
      if (isCleanSince(gen)) {
        MD.bus.emit('doc:saved');
        MD.toast('已保存');
      } else {
        // 写盘期间又有输入：磁盘是旧快照，不能清脏，留给下一次自动保存补写
        MD.toast('保存期间文档又有修改，已保留未保存状态');
      }
    } catch (e) { MD.toast('保存失败：' + e.message); }
  }
  async function saveAs() {
    const name = state.currentFile ? U.baseName(state.currentFile) : '未命名.md';
    const p = await MD.fsx.pickSaveFile(name, [
      { name: 'Markdown', extensions: ['md', 'markdown'] },
    ]);
    if (!p) return;
    const gen = MD.editor.cm && MD.editor.cm.changeGeneration ? MD.editor.cm.changeGeneration() : null;
    try {
      await MD.fsx.writeFile(p, MD.editor.value);
      state.currentFile = p;
      state.currentDir = U.dirName(p);
      MD.panels.pushRecent(p);
      MD.bus.emit('doc:opened', p);
      if (isCleanSince(gen)) {
        MD.bus.emit('doc:saved');
        MD.toast('已保存');
      } else {
        MD.toast('保存期间文档又有修改，已保留未保存状态');
      }
    } catch (e) { MD.toast('保存失败：' + e.message); }
  }
  function closeDoc() {
    if (state.dirty && !confirm('当前文档未保存，确定关闭吗？')) return;
    resetDoc();
  }
  /** 不做未保存确认的关闭（文件已被删除等场景由调用方保证） */
  function closeDocForce() {
    resetDoc();
    MD.toast('当前文件已被删除，已关闭');
  }
  function resetDoc() {
    state.currentFile = null;
    state.currentDir = null;
    MD.editor.value = '';
    markDirty(false);              // 清脏标记 + 状态栏 + 窗口标题
    MD.bus.emit('doc:closed');
    MD.bus.emit('doc:opened', null); // 让状态栏路径 / 标题刷新
    MD.app.renderPreview();
  }

  /* ══════════ 图片：粘贴 / 拖入 ══════════ */
  // 桌面版同一次拖放可能同时走 Tauri 原生事件和 CodeMirror 的 DOM drop，
  // 800ms 内相同内容只处理一次，避免插入两张一样的图、写两个文件。
  const _imgSeen = new Map();
  function isDuplicateImage(dataURL) {
    const key = dataURL.length + ':' + dataURL.slice(-64);
    const now = Date.now();
    for (const [k, t] of _imgSeen) { if (now - t > 800) _imgSeen.delete(k); }
    if (_imgSeen.has(key)) return true;
    _imgSeen.set(key, now);
    return false;
  }

  async function handleImageDataURL(dataURL, altName) {
    if (MD.fsx.isApp && isDuplicateImage(dataURL)) return;
    const alt = (altName || 'image').replace(/\.[^.]+$/, '');
    let ext = 'png';
    const m = /^data:image\/(\w+)[;,]/.exec(dataURL);
    if (m) ext = m[1].replace('jpeg', 'jpg');

    if (MD.settings.pasteImageToLocal && state.currentDir && MD.fsx.isApp) {
      try {
        const fname = `image-${Date.now()}.${ext}`;
        const rel = `assets/${fname}`;
        const abs = U.joinPath(state.currentDir, rel.replace(/\//g, '\\'));
        await MD.fsx.writeBinary(abs, dataURL);
        MD.editor.actions.imageLink(rel, alt);
        MD.toast('图片已保存到 assets/');
        return;
      } catch (e) {
        console.warn('图片保存失败，改用内嵌', e);
      }
    }
    MD.editor.actions.imageLink(dataURL, alt);
  }

  async function handleImageFile(file) {
    const dataURL = await new Promise((res, rej) => {
      const fr = new FileReader();
      fr.onload = () => res(fr.result);
      fr.onerror = rej;
      fr.readAsDataURL(file);
    });
    await handleImageDataURL(dataURL, U.baseName(file.name || ''));
  }

  function handleDrop(dt) {
    if (!dt) return;
    const files = Array.from(dt.files || []);
    for (const f of files) {
      if (f.type && f.type.startsWith('image/')) { handleImageFile(f); continue; }
      if (U.isMarkdown(f.name) || f.name.endsWith('.txt')) {
        // Tauri 拖拽事件走 app.js 里的 path 监听；浏览器模式用 File 对象
        if (!MD.fsx.isApp) {
          const p = MD.registerBrowserFile(f);
          openFile(p);
        }
      }
    }
  }

  /* ══════════ 导出 ══════════ */
  async function cssTextOf(match) {
    // 优先从已加载的样式表提取，失败再 fetch
    try {
      for (const sheet of document.styleSheets) {
        if (sheet.href && sheet.href.includes(match)) {
          return Array.from(sheet.cssRules).map((r) => r.cssText).join('\n');
        }
      }
    } catch (e) { /* 跨源受限则走 fetch */ }
    try { return await (await fetch(match)).text(); } catch (e) { return ''; }
  }

  async function getDocCssText() {
    return cssTextOf('css/doc.css');
  }

  /** 把 @font-face 里的 woff2 转成 data URI（导出文件脱离原目录也能正确显示公式） */
  async function inlineWoff2(css) {
    const files = Array.from(new Set((css.match(/fonts\/[A-Za-z0-9_.-]+\.woff2/g) || [])));
    if (!files.length) return css;
    let out = css;
    for (const f of files) {
      try {
        const res = await fetch('vendor/katex/' + f);
        if (!res.ok) continue;
        const buf = new Uint8Array(await res.arrayBuffer());
        let bin = '';
        const CHUNK = 0x8000;
        for (let i = 0; i < buf.length; i += CHUNK) {
          bin += String.fromCharCode.apply(null, buf.subarray(i, i + CHUNK));
        }
        const uri = `data:font/woff2;base64,${btoa(bin)}`;
        out = out.split(f).join(uri);
      } catch (e) { /* 浏览器模式可能拿不到字库，退化为系统字体 */ }
    }
    return out;
  }

  async function getExportCss() {
    const hljs = await cssTextOf(MD.theme.isDark(MD.settings.docTheme) ? 'hljs/github-dark.min.css' : 'hljs/github.min.css');
    const katexCss = await inlineWoff2(await cssTextOf('katex/katex.min.css'));
    return { hljs, katex: katexCss };
  }

  /** 导出前确保预览是最新的（renderPreview 是防抖的，不能"调用完就取 DOM"） */
  async function ensureRendered() {
    const preview = U.$('#preview');
    await MD.md.render(MD.editor.value, preview, state.currentDir);
    preview.hidden = MD.editor.value.trim() === '' && !state.currentFile;
  }

  /**
   * 把 `var(--x)` 递归解析成字面值。
   * Word 的 HTML 引擎（以及多数"打开 .doc"的软件）不支持 CSS 自定义属性，
   * 原样导出会让整份文档丢掉字体、字号、行距、对齐等全部版式，
   * 退回 Word 默认的宋体/Times 12pt。
   */
  function flattenVars(css, vars) {
    const refRe = /var\(\s*(--[\w-]+)\s*(?:,\s*([^()]*))?\)/g;
    const flat = Object.assign({}, vars);
    for (let round = 0; round < 4; round++) {
      let changed = false;
      for (const k of Object.keys(flat)) {
        const v = String(flat[k] == null ? '' : flat[k]);
        const nv = v.replace(refRe, (m, n, fb) => (flat[n] != null ? String(flat[n]) : (fb || '')));
        if (nv !== v) { flat[k] = nv; changed = true; }
      }
      if (!changed) break;
    }
    return css.replace(refRe, (m, n, fb) => (flat[n] != null ? String(flat[n]) : (fb || '')));
  }

  /** Word 只认 `@page WordSection1` 这一套页面设置 */
  function wordPageCss() {
    const s = MD.settings;
    const paperKey = PAPER_MM[s.paper] ? s.paper : 'A4';
    const [wmm, hmm] = PAPER_MM[paperKey];
    const m = s.margins || { top: 20, right: 18, bottom: 20, left: 18 };
    return `@page WordSection1 { size: ${wmm}mm ${hmm}mm; margin: ${m.top}mm ${m.right}mm ${m.bottom}mm ${m.left}mm; }
div.WordSection1 { page: WordSection1; }`;
  }

  async function buildStandaloneHTML(wordMode) {
    const preview = U.$('#preview');
    const { vars, extraCSS, name, pageCss, paper } = MD.theme.resolvedVars();
    const varCss = Object.entries(vars).map(([k, v]) => `${k}:${v};`).join('');
    const body = preview.innerHTML;
    const now = new Date().toLocaleString('zh-CN');
    const docCss = MD.__docCssText || await getDocCssText();
    const { hljs, katex } = await getExportCss();
    const title = U.baseName(state.currentFile || '文档').replace(/\.[^.]+$/, '');

    let css = `${wordMode ? '' : `:root { ${varCss} }
`}${docCss}
${hljs}
${katex}
${extraCSS}
body { margin: 0; background: var(--doc-bg); color: var(--doc-fg); }
/* 版式完全交给 --doc-* 变量，这里不再硬编码宽度和内边距 */
.markdown-body { margin: 0 auto; box-shadow: none; ${paper !== 'flow' ? 'width: var(--doc-page-width);' : ''} }
${wordMode ? wordPageCss() : pageCss}
@media print {
  .markdown-body { max-width: none; padding: 0; }
  .markdown-body pre, .markdown-body code, .markdown-body blockquote,
  .markdown-body th, .markdown-body td, .markdown-body mark {
    background: #fff !important; color: #000 !important;
  }
}`;
    // Word 模式：变量全部拍平成字面值，否则版式全丢
    if (wordMode) css = flattenVars(css, vars);

    return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="Generator" content="MDeditor">
<title>${U.esc(title)}</title>
${wordMode ? '<meta name="ProgId" content="Word.Document">' : ''}
<style>
${css}
</style>
</head>
<body>
${wordMode ? '<div class="WordSection1">' : ''}<article class="markdown-body">${body}</article>${wordMode ? '</div>' : ''}
${wordMode ? '' : `<div style="text-align:center;color:#9ca3af;font-size:12px;margin:30px 0">由 MDeditor 导出 · ${U.esc(now)}</div>`}
</body>
</html>`;
  }

  async function exportHTML() {
    await ensureRendered();
    const html = await buildStandaloneHTML(false);
    const name = (state.currentFile ? U.baseName(state.currentFile).replace(/\.[^.]+$/, '') : '文档') + '.html';
    const p = await MD.fsx.pickSaveFile(name, [{ name: 'HTML', extensions: ['html'] }]);
    if (!p) return;
    await MD.fsx.writeFile(p, html);
    MD.toast('已导出 HTML');
  }
  async function exportWord() {
    await ensureRendered();
    const html = await buildStandaloneHTML(true);
    const name = (state.currentFile ? U.baseName(state.currentFile).replace(/\.[^.]+$/, '') : '文档') + '.doc';
    const p = await MD.fsx.pickSaveFile(name, [{ name: 'Word 文档', extensions: ['doc'] }]);
    if (!p) return;
    await MD.fsx.writeFile(p, html);
    MD.toast('已导出 Word（.doc），可用 Word / WPS 打开');
  }
  /**
   * Markdown → 可读纯文本。
   * 原先直接把 .md 源码原样写进 .txt，用户拿到的是 `## 标题` `**加粗**` `|---|`，
   * 等于换了扩展名的源码，谈不上"纯文本"。这里按 marked 的 token 流拍平：
   * 去记号、保留标题/列表/表格/代码块的结构，图片降级为 [图片：alt]。
   */
  function toPlainText(md) {
    const out = [];
    const blank = () => { if (out.length && out[out.length - 1] !== '') out.push(''); };
    const inlineText = (toks) => (toks || []).map((t) => {
      if (!t) return '';
      if (t.type === 'image') return t.text ? `[图片：${t.text}]` : '[图片]';
      if (t.type === 'br') return '\n';
      if (t.type === 'codespan') return t.text || '';
      if (t.tokens && t.tokens.length) return inlineText(t.tokens);
      return t.text != null ? t.text : '';
    }).join('');

    const walkList = (list, depth) => {
      let n = 0;
      for (const item of list.items || []) {
        let head = '';
        const subs = [];
        for (const tok of item.tokens || []) {
          if (tok.type === 'list') { subs.push(tok); continue; }
          const piece = (tok.type === 'text' || tok.type === 'paragraph')
            ? inlineText(tok.tokens || [tok])
            : (tok.text || '');
          if (piece) head += (head ? ' ' : '') + piece;
        }
        n += 1;
        const mark = list.ordered ? `${(list.start || 1) + n - 1}. ` : '• ';
        out.push('    '.repeat(depth) + mark + head.trim());
        for (const s of subs) walkList(s, depth + 1);
      }
    };

    const walk = (tokens) => {
      for (const t of tokens) {
        switch (t.type) {
          case 'heading':
          case 'paragraph':
            out.push(inlineText(t.tokens));
            blank();
            break;
          case 'text':
            if (t.tokens) { out.push(inlineText(t.tokens)); blank(); }
            break;
          case 'blockquote':
            walk(t.tokens || []);
            break;
          case 'list':
            walkList(t, 0);
            blank();
            break;
          case 'code':
            out.push(t.text || '');
            blank();
            break;
          case 'table': {
            const head = (t.header || []).map((c) => inlineText(c.tokens || [c])).join('\t');
            if (head) out.push(head);
            for (const row of t.rows || []) out.push(row.map((c) => inlineText(c.tokens || [c])).join('\t'));
            blank();
            break;
          }
          case 'hr':
            out.push('────────────');
            blank();
            break;
          case 'space':
            blank();
            break;
          case 'html':
            break;
          default:
            if (t.text) { out.push(t.text); blank(); }
        }
      }
    };

    try {
      const lexer = window.marked && window.marked.lexer;
      if (!lexer) return md;
      // YAML 元信息不是正文，纯文本里不要
      const body = MD.md && MD.md.stripFrontMatter ? MD.md.stripFrontMatter(md) : md;
      walk(lexer(body));
    } catch (e) {
      console.warn('纯文本转换失败，回退为原文', e);
      return md;
    }
    return out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
  }

  async function exportTXT() {
    const name = (state.currentFile ? U.baseName(state.currentFile).replace(/\.[^.]+$/, '') : '文档') + '.txt';
    const p = await MD.fsx.pickSaveFile(name, [{ name: '文本文件', extensions: ['txt'] }]);
    if (!p) return;
    await MD.fsx.writeFile(p, toPlainText(MD.editor.value));
    MD.toast('已导出纯文本');
  }
  async function exportTheme() {
    const t = JSON.parse(JSON.stringify(MD.theme.getTheme(MD.settings.docTheme)));
    const name = t.name + '.json';
    const p = await MD.fsx.pickSaveFile(name, [{ name: 'JSON', extensions: ['json'] }]);
    if (!p) return;
    await MD.fsx.writeFile(p, JSON.stringify(t, null, 2));
    MD.toast('主题已导出');
  }

  /* 纸张规格（mm），与 theme.js 的 PAPERS 保持一致；PDF 导出需要换算成英寸 */
  const PAPER_MM = {
    A4: [210, 297], A5: [148, 210], B5: [176, 250], letter: [216, 279], '16k': [184, 260],
  };
  const MM_PER_IN = 25.4;

  /** 切到"打印态"：缩放回 100%；分页预览时临时把 @page 页边距归零（版式已烘进页盒） */
  async function enterPrintState() {
    const zBefore = MD.zoom ? MD.zoom.get() : 100;
    if (zBefore !== 100) await MD.zoom.set(100, { silent: true, persist: false });
    const pageStyle = U.$('#page-style');
    const pagedOn = !!(pageStyle && MD.settings.pagedPreview && MD.settings.paper !== 'flow');
    const saved = pageStyle ? pageStyle.textContent : '';
    if (pagedOn) pageStyle.textContent = '@page { size: auto; margin: 0; }';
    return () => {
      if (pagedOn && pageStyle) pageStyle.textContent = saved;
      if (zBefore !== 100) MD.zoom.set(zBefore, { silent: true, persist: false });
    };
  }

  /**
   * 把当前预览写成 PDF 文件。
   * 走 WebView2 的 PrintToPdf（Rust 侧 export_pdf），由代码显式关闭
   * 「页眉 / 页脚」，成品里不会出现日期、文档标题、tauri.localhost 与页码。
   */
  async function exportPdfTo(path, opts) {
    const o = opts || {};
    const s = MD.settings;
    const paperKey = PAPER_MM[s.paper] ? s.paper : 'A4';   // 流式模式按 A4 出纸
    const [wmm, hmm] = PAPER_MM[paperKey];
    const pagedOn = !!(s.pagedPreview && s.paper !== 'flow');
    const m = s.margins || { top: 20, right: 18, bottom: 20, left: 18 };
    // 分页预览时 paged.js 已把页边距做成页面内边距，settings 再给边距会变成双重留白
    const edge = pagedOn ? { top: 0, right: 0, bottom: 0, left: 0 } : m;

    const restore = await enterPrintState();
    if (!o.silent) MD.toast('正在生成 PDF…', 8000);
    try {
      await MD.fsx.exportPdf(path, {
        width_in: wmm / MM_PER_IN,
        height_in: hmm / MM_PER_IN,
        margin_top_in: (edge.top || 0) / MM_PER_IN,
        margin_right_in: (edge.right || 0) / MM_PER_IN,
        margin_bottom_in: (edge.bottom || 0) / MM_PER_IN,
        margin_left_in: (edge.left || 0) / MM_PER_IN,
      });
      if (!o.silent) MD.toast('已导出 PDF：' + U.baseName(path));
      return true;
    } catch (e) {
      MD.toast('PDF 导出失败：' + String((e && e.message) || e), 6000);
      console.error(e);
      return false;
    } finally {
      restore();
    }
  }

  /** 菜单「导出 PDF」：选路径后直接出文件（不经打印对话框） */
  async function exportPDF() {
    if (!MD.fsx.isApp) { MD.toast('浏览器模式请用「打印」→ 另存为 PDF'); return; }
    await ensureRendered();
    const base = state.currentFile ? U.baseName(state.currentFile).replace(/\.[^.]+$/, '') : '文档';
    const p = await MD.fsx.pickSaveFile(base + '.pdf', [{ name: 'PDF', extensions: ['pdf'] }]);
    if (!p) return;
    await exportPdfTo(p);
  }

  /** 指定路径导出（--pdf-out 自动化 / 供外部调用） */
  async function exportPDFTo(path, opts) {
    await ensureRendered();
    return exportPdfTo(path, opts);
  }

  async function printDoc() {
    // 画面放大会把内容挤出纸面，打印期间临时回到 100%，打印结束后恢复
    const zBefore = MD.zoom ? MD.zoom.get() : 100;
    if (zBefore !== 100) await MD.zoom.set(100, { silent: true, persist: false });
    let zoomTimer = 0;
    const restoreZoom = () => {
      // 兜底定时器与 afterprint 只允许生效一次，否则会覆盖用户之后改过的设置
      clearTimeout(zoomTimer);
      window.removeEventListener('afterprint', restoreZoom);
      if (zBefore !== 100) MD.zoom.set(zBefore, { silent: true, persist: false });
    };
    window.addEventListener('afterprint', restoreZoom);
    zoomTimer = setTimeout(restoreZoom, 15000);

    // 分页预览时纸面上的版式已经由 paged.js 排好，再叠加 @page 的页边距会导致
    // 每页被二次缩放/裁切，打印期间临时改成"无页边距、由内容自己决定"
    const pageStyle = U.$('#page-style');
    const pagedOn = !!(pageStyle && MD.settings.pagedPreview && MD.settings.paper !== 'flow');
    const saved = pageStyle ? pageStyle.textContent : '';
    if (pagedOn) {
      pageStyle.textContent = '@page { size: auto; margin: 0; }';
      let pageTimer = 0;
      const restore = () => {
        clearTimeout(pageTimer);
        pageStyle.textContent = saved;
        window.removeEventListener('afterprint', restore);
      };
      window.addEventListener('afterprint', restore);
      pageTimer = setTimeout(restore, 15000);
    }
    await MD.fsx.print();
  }

  /* ══════════ paged.js 分页 ══════════ */
  let pagedLoaded = null;
  function ensurePaged() {
    if (window.PagedPolyfill || window.Paged) return Promise.resolve();
    if (pagedLoaded) return pagedLoaded;
    pagedLoaded = new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = 'vendor/pagedjs/paged.polyfill.min.js';
      s.onload = resolve;
      s.onerror = () => reject(new Error('paged.js 加载失败'));
      document.head.appendChild(s);
    });
    return pagedLoaded;
  }

  let pagedSeq = 0;
  let pagedPreviewer = null;
  function clearPagedStyles() {
    document.querySelectorAll('style[data-pagedjs-inserted-styles]').forEach((n) => n.remove());
  }
  async function renderPaged(html) {
    await ensurePaged();
    const seq = ++pagedSeq;
    const output = U.$('#paged-output');
    output.hidden = false;
    output.innerHTML = '<div style="padding:40px;color:#9ca3af">分页渲染中…</div>';
    try {
      const css = [];
      const docCss = await getDocCssText();
      if (docCss) css.push(docCss);
      // paged.js 不会读取页面上已有的 <style>，@page 必须显式传进去，
      // 否则分页预览永远是它内置的 Letter 纸 + 0 边距，用户设的纸张/页边距全失效。
      const pageStyle = U.$('#page-style');
      if (pageStyle && pageStyle.textContent) css.push(pageStyle.textContent);
      if (seq !== pagedSeq) return;                  // 已有更新的渲染请求
      clearPagedStyles();                            // 反复编辑不能累积 <style>
      if (!pagedPreviewer) pagedPreviewer = new window.Paged.Previewer();
      await pagedPreviewer.preview(html, css, output);
      if (seq !== pagedSeq) return;
      // pagedjs 完成后清掉占位
      const ph = output.querySelector('div');
      if (ph && !ph.closest('.pagedjs_pages')) ph.remove();
      // paged 注入的基础样式里带 @page(size:letter)，把它压到用户 @page 之前，
      // 否则打印/导出 PDF 会莫名变成 Letter + 零边距。
      if (pageStyle) document.head.appendChild(pageStyle);
    } catch (e) {
      output.innerHTML = `<div style="padding:30px;color:#dc2626">分页渲染失败：${U.esc(String(e.message || e))}</div>`;
    }
  }

  MD.actions = {
    newFile, newFileIn: newFile, openFileDialog, openFolderDialog, openFile,
    importFile, importAsMarkdown, isConvertible,
    save, saveAs, closeDoc, closeDocForce, handleImageFile, handleImageDataURL, handleDrop,
    exportPDF, exportPDFTo, exportPdfTo, printDoc, exportHTML, exportWord, exportTXT, exportTheme,
    toPlainText, flattenVars,
    buildStandaloneHTML, getDocCssText, renderPaged, clearPagedStyles, ensureRendered,
  };

  // 启动时缓存 doc.css 文本（导出用）
  MD.__docCssText = '';
  document.addEventListener('DOMContentLoaded', () => { MD.actions.getDocCssText().then((t) => { MD.__docCssText = t; }); });
})();
