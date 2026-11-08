/* ══════════════ docs.js：多文档（标签页）管理 ══════════════ */
'use strict';

/*
  设计要点
  ───────────────────────────────────────────────────────────────
  1. 全局只有一个 CodeMirror 实例。非活动文档的缓冲区、光标、滚动位置、
     撤销历史都保存在内存中的 doc 记录里，切换标签时与编辑器交换内容。
  2. `MD.state.currentFile / currentDir / dirty` 始终镜像「活动文档」，
     因此导出、状态栏、文件树、排版等模块不需要感知多标签的存在。
  3. 标签栏只在「增删 / 切换」时整体重绘；脏标记只切换单个节点的 class，
     不会在每次按键时重建 DOM。
  4. 不在切换标签时询问未保存修改：非活动文档的修改原样留在内存里，
     关闭标签、退出程序时才逐个确认，这与主流编辑器的行为一致。
*/

(function () {
  const U = MD.util;
  const state = MD.state = MD.state || {};

  /** @type {Array<{id:string,path:string|null,dir:string|null,name:string,text:string,
   *   dirty:boolean,cursor:{line:number,ch:number},scroll:number,previewScroll:number,
   *   history:object|null,mtime:number|null}>} */
  const docs = [];
  let activeId = null;
  let seq = 0;
  let untitledSeq = 0;

  const byId = (id) => (id ? docs.find((d) => d.id === id) || null : null);
  const active = () => byId(activeId);

  /** Windows 路径大小写不敏感；同一个文件不能开两个标签（否则两处保存互相覆盖） */
  function samePath(a, b) {
    if (!a || !b) return false;
    if (a === b) return true;
    return (MD.fsx.sep || '\\') === '\\' && String(a).toLowerCase() === String(b).toLowerCase();
  }

  function findByPath(p) {
    return p ? docs.find((d) => samePath(d.path, p)) || null : null;
  }

  function makeDoc(o) {
    seq += 1;
    return {
      id: 'doc' + seq,
      path: o.path || null,
      dir: o.dir || (o.path ? U.dirName(o.path) : null),
      name: o.name || (o.path ? U.baseName(o.path) : '未命名.md'),
      text: typeof o.text === 'string' ? o.text : '',
      dirty: false,
      cursor: { line: 0, ch: 0 },
      scroll: 0,
      previewScroll: 0,
      history: null,
      mtime: o.mtime == null ? null : o.mtime,
    };
  }

  async function mtimeOf(p) {
    if (!MD.fsx.isApp || !MD.fsx.fileMtime || !p) return null;
    try { return await MD.fsx.fileMtime(p); } catch (e) { return null; }
  }

  /* ══════════ 编辑器 ↔ 文档记录 ══════════ */

  /** 把编辑器里的当前状态（内容 / 脏标记 / 光标 / 滚动 / 撤销历史）存回活动文档 */
  function snapshot() {
    const d = active();
    const cm = MD.editor.cm;
    if (!d || !cm) return;
    d.text = cm.getValue();
    d.dirty = !!state.dirty;
    try { d.cursor = cm.getCursor(); } catch (e) { /* ignore */ }
    try {
      const si = cm.getScrollInfo();
      d.scroll = si ? si.top : 0;
    } catch (e) { d.scroll = 0; }
    // 撤销历史按文档保存，否则切回来 Ctrl+Z 就失效。超过 2MB 的文档不存，避免内存翻倍。
    if (cm.getHistory && d.text.length < 2 * 1024 * 1024) {
      try { d.history = cm.getHistory(); } catch (e) { d.history = null; }
    } else {
      d.history = null;
    }
    const pv = U.$('#preview-scroll');
    if (pv) d.previewScroll = pv.scrollTop;
  }

  /** 把文档记录载入编辑器（不触发 doc:changed，不污染撤销栈） */
  function loadBuffer(d) {
    MD.editor.value = d.text;          // setValue + clearHistory + markClean
    const cm = MD.editor.cm;
    if (!cm) return;
    if (d.history && cm.setHistory) {
      try { cm.setHistory(d.history); cm.markClean(); } catch (e) { d.history = null; }
    }
    // 光标 / 滚动要等 CodeMirror 重新测量后再设，否则长文档会按旧行数截断
    requestAnimationFrame(() => {
      if (active() !== d) return;
      try {
        const line = U.clamp(d.cursor.line || 0, 0, cm.lastLine());
        const ch = U.clamp(d.cursor.ch || 0, 0, (cm.getLine(line) || '').length);
        cm.setCursor({ line, ch });
        cm.refresh();
        cm.scrollTo(null, d.scroll || 0);
      } catch (e) { /* ignore */ }
      restorePreviewScroll(d.previewScroll, d);
    });
  }

  /** 预览滚动：等这次渲染完成后再设，避免内容还没排好就被 clamp 到顶部 */
  function restorePreviewScroll(top, doc) {
    if (!top) return;
    const pv = U.$('#preview-scroll');
    if (!pv) return;
    let done = false;
    const apply = () => {
      if (done) return;
      done = true;
      MD.bus.off('render:done', apply);
      if (doc && active() !== doc) return;   // 期间又切了标签：别把滚动位置套到别的文档上
      pv.scrollTop = top;
    };
    MD.bus.on('render:done', apply);
    setTimeout(apply, 500);            // 内容没变化时不会有新的 render:done
  }

  /** 切标签后让状态栏 / 标题 / 标签脏点与活动文档一致 */
  function syncChrome() {
    const d = active();
    if (MD.actions && MD.actions.markDirty) MD.actions.markDirty(!!(d && d.dirty));
  }

  /* ══════════ 标签栏渲染 ══════════ */
  function tabHTML(d) {
    const on = d.id === activeId;
    const tip = d.path || d.name + '（尚未保存）';
    return '<button class="tab' + (on ? ' active' : '') + (d.dirty ? ' dirty' : '') + '"'
      + ' data-tab="' + d.id + '" role="tab" aria-selected="' + (on ? 'true' : 'false') + '"'
      + ' title="' + U.esc(tip) + '">'
      + '<span class="tab-dot" aria-hidden="true"></span>'
      + '<span class="tab-title">' + U.esc(d.name) + '</span>'
      + '<span class="tab-close" data-tab-close="' + d.id + '" title="关闭此标签页">✕</span>'
      + '</button>';
  }

  function render() {
    const bar = U.$('#tabbar');
    const list = U.$('#tab-list');
    if (!bar || !list) return;
    bar.hidden = docs.length === 0;
    const keep = list.scrollLeft;
    list.innerHTML = docs.map(tabHTML).join('');
    list.scrollLeft = keep;
    ensureVisible(activeId);
    renderMenu();
  }

  function ensureVisible(id) {
    const list = U.$('#tab-list');
    if (!list || !id) return;
    const el = list.querySelector('[data-tab="' + id + '"]');
    if (!el) return;
    const left = el.offsetLeft;
    const right = left + el.offsetWidth;
    if (left < list.scrollLeft) list.scrollLeft = Math.max(0, left - 8);
    else if (right > list.scrollLeft + list.clientWidth) list.scrollLeft = right - list.clientWidth + 8;
  }

  /** 标签太多时的下拉列表：一屏看不到的标签都能从这里切 */
  function renderMenu() {
    const menu = U.$('#tab-menu');
    if (!menu) return;
    if (!docs.length) {
      menu.innerHTML = '<div class="menu-label">没有打开的文件</div>';
      return;
    }
    menu.innerHTML = docs.map((d) => (
      '<button data-tab-jump="' + d.id + '" class="' + (d.id === activeId ? 'checked' : '') + '"'
      + ' title="' + U.esc(d.path || d.name) + '">'
      + '<span class="tab-menu-name">' + U.esc(d.name) + (d.dirty ? ' ●' : '') + '</span>'
      + '</button>'
    )).join('');
  }

  function toggleMenu(e) {
    const menu = U.$('#tab-menu');
    const btn = U.$('#tab-more');
    if (!menu || !btn) return;
    if (e) e.stopPropagation();
    if (!menu.hidden) { menu.hidden = true; return; }
    renderMenu();
    if (menu.parentElement !== document.body) document.body.appendChild(menu);
    menu.hidden = false;
    const r = btn.getBoundingClientRect();
    menu.style.top = (r.bottom + 6) + 'px';
    const left = Math.min(r.right - menu.offsetWidth, window.innerWidth - menu.offsetWidth - 12);
    menu.style.left = Math.max(8, left) + 'px';
  }

  /* ══════════ 打开 / 切换 / 关闭 ══════════ */

  /** 只压入文档记录，不切换活动文档（会话恢复用） */
  function insert(o) {
    const d = makeDoc(o);
    docs.push(d);
    return d;
  }

  /** 新建一个标签并切过去 */
  function add(o) {
    const d = insert(o);
    activate(d.id, { noRecheck: true });
    return d;
  }

  function activate(id, opts) {
    const d = byId(id);
    if (!d) return false;
    if (activeId === id) { render(); return true; }
    snapshot();                        // 先把旧文档的缓冲区存回去
    activeId = id;
    state.currentFile = d.path;
    state.currentDir = d.dir || (d.path ? U.dirName(d.path) : null);
    state.dirty = !!d.dirty;
    loadBuffer(d);
    syncChrome();
    render();
    if (MD.app && MD.app.renderPreview) MD.app.renderPreview();
    MD.bus.emit('doc:opened', d.path);
    if (!(opts && opts.noRecheck)) recheck(d);
    saveSession();
    return true;
  }

  function closeActive(opts) {
    return close(activeId, opts);
  }

  function close(id, opts) {
    const o = opts || {};
    const d = byId(id);
    if (!d) return false;
    if (d.id === activeId) snapshot();
    if (d.dirty && !o.force) {
      if (!confirm('「' + d.name + '」有未保存的修改，确定关闭吗？')) return false;
    }
    const idx = docs.indexOf(d);
    docs.splice(idx, 1);
    let remaining = docs.length;
    if (d.id === activeId) {
      activeId = null;
      const next = docs[Math.min(idx, docs.length - 1)];
      if (next) activate(next.id);
      else clearAll();
    } else {
      render();
    }
    saveSession();
    MD.bus.emit('doc:closed', { path: d.path, remaining });
    return true;
  }

  function closeByPath(p, opts) {
    const d = findByPath(p);
    return d ? close(d.id, opts) : false;
  }

  /** 最后一个标签关掉：清空编辑器并回到欢迎页 */
  function clearAll() {
    state.currentFile = null;
    state.currentDir = null;
    state.dirty = false;
    if (MD.editor.cm) MD.editor.value = '';
    syncChrome();
    render();
    MD.bus.emit('doc:opened', null);
    if (MD.app && MD.app.renderPreview) MD.app.renderPreview();
  }

  /** Ctrl+Tab / Ctrl+PageDown 轮换 */
  function cycle(step) {
    if (docs.length < 2) return;
    const i = docs.findIndex((d) => d.id === activeId);
    const n = docs[((i < 0 ? 0 : i) + (step || 1) + docs.length) % docs.length];
    if (n) activate(n.id);
  }

  function newDoc(dir) {
    untitledSeq += 1;
    const name = untitledSeq > 1 ? '未命名 ' + untitledSeq + '.md' : '未命名.md';
    const text = MD.defaults.newDocTemplate || '# 未命名文档\n\n';
    return add({ path: null, dir: dir || null, name, text });
  }

  /* ══════════ 脏标记 / 保存后的同步 ══════════ */

  /** 由 actions.markDirty 调用：只在真正变化时改 DOM，避免每次按键重建 */
  function setActiveDirty(on) {
    const d = active();
    if (!d) return;
    const v = !!on;
    if (d.dirty === v) return;
    d.dirty = v;
    const el = U.$('#tab-list [data-tab="' + d.id + '"]');
    if (el) el.classList.toggle('dirty', v);
    const item = U.$('#tab-menu [data-tab-jump="' + d.id + '"] .tab-menu-name');
    if (item) item.textContent = d.name + (v ? ' ●' : '');
  }

  function setDirtyById(id, on) {
    const d = byId(id);
    if (!d) return;
    if (d.id === activeId) { setActiveDirty(on); return; }
    d.dirty = !!on;
    const el = U.$('#tab-list [data-tab="' + d.id + '"]');
    if (el) el.classList.toggle('dirty', !!on);
    const item = U.$('#tab-menu [data-tab-jump="' + d.id + '"] .tab-menu-name');
    if (item) item.textContent = d.name + (on ? ' ●' : '');
  }

  /** 写盘成功后同步 mtime，避免下次激活时误判成「外部修改」 */
  async function afterWrite(d) {
    if (!d) return;
    d.mtime = await mtimeOf(d.path);
  }

  function renamePath(id, to) {
    const d = byId(id);
    if (!d || !to) return;
    const from = d.path;
    d.path = to;
    d.dir = U.dirName(to);
    d.name = U.baseName(to);
    d.mtime = null;
    if (d.id === activeId) { state.currentFile = to; state.currentDir = d.dir; }
    render();
    saveSession();
    afterWrite(d);
    if (d.id === activeId) MD.bus.emit('doc:renamed', { from, to });
  }

  /** 文件在磁盘上被重命名（文件树操作） */
  function onExternalRename(from, to) {
    const d = findByPath(from);
    if (!d || !to) return;
    d.path = to;
    d.dir = U.dirName(to);
    d.name = U.baseName(to);
    d.mtime = null;
    if (d.id === activeId) { state.currentFile = to; state.currentDir = d.dir; }
    render();
    saveSession();
    afterWrite(d);
  }

  /** 用磁盘内容覆盖某个标签的缓冲区（外部改写 / 重新导入后用） */
  function reloadBuffer(id, text, mtime) {
    const d = byId(id);
    if (!d) return false;
    d.text = text;
    d.dirty = false;
    d.history = null;                 // 旧历史对新内容无效，必须丢掉
    if (mtime != null) d.mtime = mtime;
    if (d.id === activeId) {
      loadBuffer(d);
      syncChrome();
      if (MD.app && MD.app.renderPreview) MD.app.renderPreview();
    } else {
      setDirtyById(d.id, false);
    }
    return true;
  }

  /* ══════════ 外部修改：激活时补一次一致性检查 ══════════ */
  async function recheck(d) {
    if (!MD.fsx.isApp || !d.path || d.dirty || d.mtime == null) return;
    const m = await mtimeOf(d.path);
    if (m == null || m === d.mtime) return;
    let text;
    try { text = await MD.fsx.readFile(d.path); } catch (e) { return; }
    if (active() !== d) {             // 读取期间又切走了：只更新记录，不动编辑器
      d.text = text;
      d.mtime = m;
      return;
    }
    reloadBuffer(d.id, text, m);
    MD.toast('「' + d.name + '」已在外部更新，已重新加载');
  }

  /* ══════════ 会话持久化 ══════════ */
  function saveSession() {
    if (!MD.settings) return;
    const a = active();
    // 只记录路径，不记录内容；上限 40 条，避免设置项无限膨胀
    MD.settings.openDocs = docs.filter((d) => d.path).map((d) => d.path).slice(0, 40);
    MD.settings.activeDoc = a && a.path ? a.path : '';
    MD.saveSettings();
  }

  /** 启动时恢复上次打开的标签（只恢复真实存在且可读的文件） */
  async function restore(paths, activePath) {
    const list = (paths || []).filter((p) => typeof p === 'string' && p);
    if (!list.length) return 0;
    const loaded = [];
    for (const p of list) {
      try {
        if (MD.fsx.isApp && !(await MD.fsx.exists(p))) continue;
        const text = await MD.fsx.readFile(p);
        loaded.push({ path: p, text, mtime: await mtimeOf(p) });
      } catch (e) { /* 单个文件读失败不影响其它标签 */ }
    }
    if (!loaded.length) return 0;
    for (const o of loaded) insert(o);
    const target = loaded.find((o) => samePath(o.path, activePath)) || loaded[loaded.length - 1];
    const d = findByPath(target.path);
    if (d) activate(d.id, { noRecheck: true });
    syncChrome();
    return loaded.length;
  }

  /* ══════════ 查询 ══════════ */
  function anyDirty() {
    return docs.some((d) => d.dirty || (d.id === activeId && state.dirty));
  }
  function dirtyDocs() {
    return docs.filter((d) => d.dirty || (d.id === activeId && state.dirty));
  }
  function count() { return docs.length; }

  /* ══════════ 事件绑定 ══════════ */
  function init() {
    const list = U.$('#tab-list');
    const menu = U.$('#tab-menu');
    if (list) {
      list.addEventListener('click', (e) => {
        const x = e.target.closest('[data-tab-close]');
        if (x) { e.stopPropagation(); close(x.dataset.tabClose); return; }
        const t = e.target.closest('[data-tab]');
        if (t) activate(t.dataset.tab);
      });
      // 中键关闭：和浏览器 / PDF 阅读器的手感一致
      list.addEventListener('mousedown', (e) => {
        if (e.button !== 1) return;
        const t = e.target.closest('[data-tab]');
        if (!t) return;
        e.preventDefault();
        close(t.dataset.tab);
      });
      // 按住 Shift 滚轮横向滚动标签栏
      list.addEventListener('wheel', (e) => {
        if (!e.deltaY || e.deltaX) return;
        e.preventDefault();
        list.scrollLeft += e.deltaY;
      }, { passive: false });
    }
    const newBtn = U.$('#tab-newfile');
    if (newBtn) newBtn.addEventListener('click', () => MD.actions.newFile());
    const moreBtn = U.$('#tab-more');
    if (moreBtn) moreBtn.addEventListener('click', (e) => toggleMenu(e));
    if (menu) {
      menu.addEventListener('click', (e) => {
        const b = e.target.closest('[data-tab-jump]');
        if (!b) return;
        menu.hidden = true;
        activate(b.dataset.tabJump);
      });
    }
    document.addEventListener('click', (e) => {
      if (!menu || menu.hidden) return;
      if (e.target.closest('#tab-menu') || e.target.closest('#tab-more')) return;
      menu.hidden = true;
    });
    render();
  }

  MD.docs = {
    init, render, add, insert, activate, close, closeActive, closeByPath, cycle, newDoc,
    findByPath, byId, active, count, anyDirty, dirtyDocs, snapshot,
    setActiveDirty, setDirtyById, afterWrite, renamePath, onExternalRename, reloadBuffer,
    mtimeOf, saveSession, restore, samePath,
    get activeId() { return activeId; },
    displayName() { const d = active(); return d ? d.name : ''; },
  };
})();
