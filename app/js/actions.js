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
    S().lastFolder = p;
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
      MD.toast('已转换：' + U.baseName(res));
      await openFile(res);
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
        MD.panels.setFolder(U.dirName(path));
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
  async function save() {
    if (!state.currentFile) return saveAs();
    try {
      await MD.fsx.writeFile(state.currentFile, MD.editor.value);
      MD.bus.emit('doc:saved');
      MD.toast('已保存');
    } catch (e) { MD.toast('保存失败：' + e.message); }
  }
  async function saveAs() {
    const name = state.currentFile ? U.baseName(state.currentFile) : '未命名.md';
    const p = await MD.fsx.pickSaveFile(name, [
      { name: 'Markdown', extensions: ['md', 'markdown'] },
    ]);
    if (!p) return;
    try {
      await MD.fsx.writeFile(p, MD.editor.value);
      state.currentFile = p;
      state.currentDir = U.dirName(p);
      MD.panels.pushRecent(p);
      MD.bus.emit('doc:opened', p);
      MD.bus.emit('doc:saved');
      MD.toast('已保存');
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

  async function buildStandaloneHTML(wordMode) {
    const preview = U.$('#preview');
    const { vars, extraCSS, name, pageCss, paper } = MD.theme.resolvedVars();
    const varCss = Object.entries(vars).map(([k, v]) => `${k}:${v};`).join('');
    const body = preview.innerHTML;
    const now = new Date().toLocaleString('zh-CN');
    const docCss = MD.__docCssText || await getDocCssText();
    const { hljs, katex } = await getExportCss();
    return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="author" content="行一">
<meta name="generator" content="MDeditor">
<meta name="description" content="由 MDeditor 导出；作者 行一，公众号「每天AI进步」">
<title>${U.esc(U.baseName(state.currentFile || '文档'))}</title>
${wordMode ? '<meta name="ProgId" content="Word.Document"><meta name="Generator" content="Microsoft Word">' : ''}
<style>
:root { ${varCss} }
${docCss}
${hljs}
${katex}
${extraCSS}
body { margin: 0; background: var(--doc-bg); color: var(--doc-fg); }
/* 版式完全交给 --doc-* 变量，这里不再硬编码宽度和内边距 */
.markdown-body { margin: 0 auto; box-shadow: none; ${paper !== 'flow' ? 'width: var(--doc-page-width);' : ''} }
${pageCss}
@media print {
  .markdown-body { max-width: none; padding: 0; }
  .markdown-body pre, .markdown-body code, .markdown-body blockquote,
  .markdown-body th, .markdown-body td, .markdown-body mark {
    background: #fff !important; color: #000 !important;
  }
}
</style>
</head>
<body>
<article class="markdown-body">${body}</article>
${wordMode ? '' : `<div style="text-align:center;color:#9ca3af;font-size:12px;margin:30px 0">${U.esc(name)} · ${U.esc(now)}</div>`}
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
  async function exportTXT() {
    const name = (state.currentFile ? U.baseName(state.currentFile).replace(/\.[^.]+$/, '') : '文档') + '.txt';
    const p = await MD.fsx.pickSaveFile(name, [{ name: '文本文件', extensions: ['txt'] }]);
    if (!p) return;
    await MD.fsx.writeFile(p, MD.editor.value);
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

  async function printDoc() {
    // 画面放大会把内容挤出纸面，打印期间临时回到 100%，打印结束后恢复
    const zBefore = MD.zoom ? MD.zoom.get() : 100;
    if (zBefore !== 100) await MD.zoom.set(100, { silent: true, persist: false });
    const restoreZoom = () => {
      window.removeEventListener('afterprint', restoreZoom);
      if (zBefore !== 100) MD.zoom.set(zBefore, { silent: true, persist: false });
    };
    window.addEventListener('afterprint', restoreZoom);
    setTimeout(restoreZoom, 15000);

    // 分页预览时纸面上的版式已经由 paged.js 排好，再叠加 @page 的页边距会导致
    // 每页被二次缩放/裁切，打印期间临时改成"无页边距、由内容自己决定"
    const pageStyle = U.$('#page-style');
    const pagedOn = !!(pageStyle && MD.settings.pagedPreview && MD.settings.paper !== 'flow');
    const saved = pageStyle ? pageStyle.textContent : '';
    if (pagedOn) {
      pageStyle.textContent = '@page { size: auto; margin: 0; }';
      const restore = () => {
        pageStyle.textContent = saved;
        window.removeEventListener('afterprint', restore);
      };
      window.addEventListener('afterprint', restore);
      setTimeout(restore, 15000);
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
    exportPDF: printDoc, exportHTML, exportWord, exportTXT, exportTheme, printDoc,
    buildStandaloneHTML, getDocCssText, renderPaged, clearPagedStyles, ensureRendered,
  };

  // 启动时缓存 doc.css 文本（导出用）
  MD.__docCssText = '';
  document.addEventListener('DOMContentLoaded', () => { MD.actions.getDocCssText().then((t) => { MD.__docCssText = t; }); });
})();
