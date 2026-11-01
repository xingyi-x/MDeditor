/* ══════════════ panels.js：文件树 / 大纲 / 全局搜索 / 欢迎页 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  const S = () => MD.settings;
  const state = MD.state = MD.state || {};

  /* ══════════ 文件树 ══════════ */
  const expanded = new Set();

  async function renderTree() {
    const box = U.$('#filetree');
    if (!state.currentFolder) {
      box.innerHTML = '<div style="padding:14px;color:var(--ui-muted);font-size:12px;line-height:1.8">未打开文件夹。<br>点击上方“📂 文件夹”选择一个目录。</div>';
      return;
    }
    box.innerHTML = '<div style="padding:14px;color:var(--ui-muted);font-size:12px">加载中…</div>';
    try {
      const items = await MD.fsx.listDir(state.currentFolder);
      box.innerHTML = `
        <div class="tree-item" data-root="1" title="${U.esc(state.currentFolder)}">
          <span class="ficon">▣</span><span class="fname">${U.esc(U.baseName(state.currentFolder) || state.currentFolder)}</span>
        </div>
        ${renderLevel(items, state.currentFolder)}
      `;
      bindTreeEvents(box);
    } catch (e) {
      box.innerHTML = `<div style="padding:14px;color:var(--ui-danger);font-size:12px">读取失败：${U.esc(e.message)}</div>`;
    }
  }

  function renderLevel(items, dirPath) {
    const dirs = items.filter((i) => i.isDir && !i.name.startsWith('.'));
    const files = items.filter((i) => !i.isDir && U.isMarkdown(i.name));
    let html = '';
    for (const d of dirs) {
      const open = expanded.has(d.path);
      html += `
        <div class="tree-item dir ${open ? 'dir-open' : ''}" data-dir="${U.esc(d.path)}">
          <span class="twisty">▶</span><span class="ficon">▸</span><span class="fname">${U.esc(d.name)}</span>
          <span class="fbtns"><button data-dact="newfile" title="新建文件">＋</button><button data-dact="reveal" title="打开位置">↗</button></span>
        </div>
        ${open ? '<div class="tree-children" data-children="' + U.esc(d.path) + '"><div style="padding:4px;color:var(--ui-muted);font-size:11px">…</div></div>' : ''}
      `;
    }
    for (const f of files) {
      const active = state.currentFile === f.path ? ' active' : '';
      html += `
        <div class="tree-item file${active}" data-file="${U.esc(f.path)}" title="${U.esc(f.path)}（${U.bytes(f.size)}）">
          <span class="twisty" style="visibility:hidden">▶</span><span class="ficon">≡</span><span class="fname">${U.esc(f.name)}</span>
          <span class="fbtns"><button data-fact="reveal" title="打开位置">↗</button><button data-fact="rename" title="重命名">✎</button><button data-fact="delete" title="删除">✕</button></span>
        </div>
      `;
    }
    // 非目录：折叠提示
    const others = items.filter((i) => !i.isDir && !U.isMarkdown(i.name) && !i.name.startsWith('.'));
    if (!dirs.length && !files.length && others.length) {
      html += '<div style="padding:6px 8px;color:var(--ui-muted);font-size:11.5px">（无 Markdown 文件）</div>';
    }
    return html;
  }

  function bindTreeEvents(box) {
    box.querySelectorAll('.tree-item.dir').forEach((el) => {
      el.addEventListener('click', (e) => {
        if (e.target.closest('[data-dact]')) return;
        const p = el.dataset.dir;
        if (expanded.has(p)) expanded.delete(p); else expanded.add(p);
        renderTree();
      });
      el.querySelectorAll('[data-dact]').forEach((btn) => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          const p = el.dataset.dir;
          if (btn.dataset.dact === 'newfile') MD.actions.newFileIn(p);
          else if (btn.dataset.dact === 'reveal') MD.fsx.reveal(p);
        });
      });
    });
    // 懒加载展开的子目录
    box.querySelectorAll('[data-children]').forEach(async (child) => {
      const dirPath = child.dataset.children;
      try {
        const items = await MD.fsx.listDir(dirPath);
        child.innerHTML = renderLevel(items, dirPath);
        bindTreeEvents(child);
      } catch (e) {
        child.innerHTML = `<div style="padding:6px;color:var(--ui-danger);font-size:11px">${U.esc(e.message)}</div>`;
      }
    });
    box.querySelectorAll('.tree-item.file').forEach((el) => {
      el.addEventListener('click', (e) => {
        if (e.target.closest('[data-fact]')) return;
        MD.actions.openFile(el.dataset.file);
      });
      el.querySelectorAll('[data-fact]').forEach((btn) => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          const p = el.dataset.file;
          const act = btn.dataset.fact;
          if (act === 'reveal') MD.fsx.reveal(p);
          else if (act === 'rename') renameItem(p);
          else if (act === 'delete') deleteItem(p);
        });
      });
    });
  }

  async function renameItem(p) {
    const oldName = U.baseName(p);
    const newName = prompt('重命名为：', oldName);
    if (!newName || newName === oldName) return;
    try {
      const to = U.joinPath(U.dirName(p), newName);
      await MD.fsx.rename(p, to);
      if (state.currentFile === p) { state.currentFile = to; MD.bus.emit('doc:renamed', { from: p, to }); }
      await renderTree();
      MD.toast('已重命名');
    } catch (e) { MD.toast('重命名失败：' + e.message); }
  }
  async function deleteItem(p) {
    if (!confirm(`确定删除「${U.baseName(p)}」吗？此操作不可恢复。`)) return;
    try {
      await MD.fsx.remove(p);
      if (state.currentFile === p) MD.bus.emit('doc:close-request');
      await renderTree();
      MD.toast('已删除');
    } catch (e) { MD.toast('删除失败：' + e.message); }
  }

  /* ══════════ 最近打开 ══════════ */
  function renderRecent() {
    const box = U.$('#recent-list');
    const list = (S().recentFiles || []).slice(0, 12);
    if (!list.length) {
      box.innerHTML = '<div style="padding:8px 6px;color:var(--ui-muted);font-size:12px">暂无记录</div>';
      return;
    }
    box.innerHTML = list.map((f) => `
      <div class="tree-item file" data-file="${U.esc(f.path)}" title="${U.esc(f.path)}">
        <span class="ficon">≡</span><span class="fname">${U.esc(f.name)}</span>
      </div>
    `).join('');
    box.querySelectorAll('[data-file]').forEach((el) => {
      el.addEventListener('click', () => MD.actions.openFile(el.dataset.file));
    });
  }

  function pushRecent(path) {
    const list = (S().recentFiles || []).filter((f) => f.path !== path);
    list.unshift({ path, name: U.baseName(path), time: Date.now() });
    S().recentFiles = list.slice(0, 20);
    MD.saveSettings();
    renderRecent();
  }

  /* ══════════ 大纲 ══════════ */
  function renderOutline() {
    const box = U.$('#outline');
    const src = MD.editor.value;
    const items = MD.md.parseOutline(src);
    if (!items.length) {
      box.innerHTML = '<div style="padding:14px;color:var(--ui-muted);font-size:12px">没有标题。用 # 开头创建标题后即可生成大纲。</div>';
      return;
    }
    // 只取前 6 级、限制数量防卡
    const max = 400;
    box.innerHTML = items.slice(0, max).map((it, idx) => `
      <div class="out-item out-l${U.clamp(it.level, 1, 6)}" data-line="${it.line}" data-idx="${idx}" title="${U.esc(it.text)}">${U.esc(it.text)}</div>
    `).join('') + (items.length > max ? `<div style="padding:6px;color:var(--ui-muted);font-size:11px">… 共 ${items.length} 个标题</div>` : '');
    box.querySelectorAll('.out-item').forEach((el) => {
      el.addEventListener('click', () => {
        const line = parseInt(el.dataset.line);
        const idx = parseInt(el.dataset.idx);
        MD.editor.scrollToLine(line);
        jumpPreviewHeading(idx);
      });
    });
  }

  function jumpPreviewHeading(idx) {
    const preview = U.$('#preview');
    const heads = U.$$('h1,h2,h3,h4,h5,h6', preview);
    const target = heads[idx];
    if (target) target.scrollIntoView({ block: 'start', behavior: 'smooth' });
  }

  /* ══════════ 全局搜索 ══════════ */
  function initSearch() {
    const input = U.$('#gsearch-input');
    const run = U.debounce(async () => {
      const q = input.value.trim();
      const box = U.$('#gsearch-results');
      if (!q) { box.innerHTML = ''; return; }
      if (!state.currentFolder) { box.innerHTML = '<div style="padding:12px;color:var(--ui-muted);font-size:12px">请先打开一个文件夹才能搜索。</div>'; return; }
      box.innerHTML = '<div style="padding:12px;color:var(--ui-muted);font-size:12px">搜索中…</div>';
      try {
        const hits = await MD.fsx.search(state.currentFolder, q, U.$('#gsearch-case').checked);
        if (!hits.length) { box.innerHTML = '<div style="padding:12px;color:var(--ui-muted);font-size:12px">没有匹配结果</div>'; return; }
        let lastFile = '';
        let html = '';
        for (const h of hits) {
          if (h.path !== lastFile) {
            lastFile = h.path;
            html += `<div class="g-file" title="${U.esc(h.path)}">${U.esc(h.name)}</div>`;
          }
          // 先在原文上切分命中片段，再逐段转义：直接对已转义文本做替换会把 &amp; 切成 &<mark>amp</mark>;
          const caseSensitive = U.$('#gsearch-case').checked;
          const re = new RegExp('(' + escapeReg(q) + ')', caseSensitive ? 'g' : 'gi');
          const markedText = String(h.text || '').split(re)
            .map((part, i) => (i % 2 ? `<mark>${U.esc(part)}</mark>` : U.esc(part)))
            .join('');
          html += `<div class="g-hit" data-path="${U.esc(h.path)}" data-line="${h.line - 1}">行 ${h.line}：${markedText}</div>`;
        }
        box.innerHTML = html;
        box.querySelectorAll('.g-hit').forEach((el) => {
          el.addEventListener('click', async () => {
            await MD.actions.openFile(el.dataset.path);
            setTimeout(() => MD.editor.scrollToLine(parseInt(el.dataset.line)), 120);
          });
        });
      } catch (e) {
        box.innerHTML = `<div style="padding:12px;color:var(--ui-danger);font-size:12px">${U.esc(e.message)}</div>`;
      }
    }, 400);
    input.addEventListener('input', run);
    U.$('#gsearch-case').addEventListener('change', run);
  }
  function escapeReg(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

  /* ══════════ 欢迎页 ══════════ */
  function renderWelcome() {
    const w = U.$('#welcome');
    const recent = (S().recentFiles || []).slice(0, 6);
    w.innerHTML = `
      <svg viewBox="0 0 24 24" width="64" height="64"><rect x="1" y="1" width="22" height="22" rx="5" fill="#4f46e5"/><path d="M5 16.5V7.5l4 5 3-7 3 7 4-5v9" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>
      <h1>MDeditor</h1>
      <p class="sub">本地 Markdown 阅读 · 编辑 · 排版 · 导出</p>
      <div class="welcome-actions">
        <button class="btn primary" data-wact="openFile">打开文件</button>
        <button class="btn" data-wact="openFolder">打开文件夹</button>
        <button class="btn" data-wact="newFile">新建文档</button>
      </div>
      ${recent.length ? `
      <div class="w-recent">
        <h3>最近打开</h3>
        <div class="w-card">
          ${recent.map((f) => `<button class="w-item" data-wfile="${U.esc(f.path)}">${U.esc(f.name)}<span style="float:right;color:var(--ui-muted);font-size:11px">${U.time(f.time)}</span></button>`).join('')}
        </div>
      </div>` : ''}
      <div class="w-hint">
        支持 $数学公式$ · 代码高亮 · Mermaid 图表 · GFM 表格与任务列表<br>
        在「主题」中一键换肤，「排版」里调整纸张、字体与页边距，导出 PDF / HTML / Word
      </div>
      <div class="w-author">作者 <b>行一</b></div>
      <div class="w-author-sub">
        想发现更多有意思的小工具，可以关注公众号<span class="w-nowrap">「每天AI进步」</span>
      </div>
      <div class="w-legal">
        免责声明：本软件仅供个人学习、研究与日常写作使用，不得用于任何商业用途。软件按「现状」提供，
        不对适用性与数据安全作任何担保，使用前请自行备份重要文件；因使用本软件产生的任何损失、
        商业纠纷或法律责任，均由使用者自行承担，开发者不承担任何责任。
      </div>
    `;
    w.hidden = false;
    w.querySelectorAll('[data-wact]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const act = btn.dataset.wact;
        if (act === 'openFile') MD.actions.openFileDialog();
        else if (act === 'openFolder') MD.actions.openFolderDialog();
        else if (act === 'newFile') MD.actions.newFile();
      });
    });
    w.querySelectorAll('[data-wfile]').forEach((btn) => {
      btn.addEventListener('click', () => MD.actions.openFile(btn.dataset.wfile));
    });
  }
  function hideWelcome() {
    const w = U.$('#welcome');
    w.hidden = true;
    w.innerHTML = '';
  }

  /* ══════════ 初始化 ══════════ */
  function init() {
    // 侧栏 tab 切换
    U.$$('.side-tabs button').forEach((btn) => {
      btn.addEventListener('click', () => {
        U.$$('.side-tabs button').forEach((b) => b.classList.remove('active'));
        btn.classList.add('active');
        U.$$('.side-body').forEach((b) => { b.hidden = true; });
        U.$(`[data-side-body="${btn.dataset.sideTab}"]`).hidden = false;
        if (btn.dataset.sideTab === 'outline') renderOutline();
      });
    });
    // [data-act] 按钮统一由 app.js 事件委托处理，这里不重复绑定
    initSearch();
    renderTree();
    renderRecent();
    renderWelcome();
    MD.bus.on('folder:opened', renderTree);
    MD.bus.on('doc:opened', () => { hideWelcome(); renderTree(); renderOutline(); });
    MD.bus.on('doc:closed', () => { renderTree(); renderWelcome(); renderOutline(); });
    MD.bus.on('render:done', () => {
      const active = U.$('.side-tabs button.active');
      if (active && active.dataset.sideTab === 'outline') renderOutline();
    });
  }

  MD.panels = {
    init, renderTree, renderRecent, renderOutline, renderWelcome, pushRecent,
    get currentFolder() { return state.currentFolder; },
    setFolder(p) { state.currentFolder = p; MD.bus.emit('folder:opened', p); },
  };
})();
