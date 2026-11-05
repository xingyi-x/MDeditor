/* ══════════════ app.js：初始化与全局控制 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  const state = MD.state;

  /* ══════════ 渲染循环 ══════════ */
  let renderSeq = 0;
  const renderNow = U.debounce(async () => {
    const seq = ++renderSeq;
    const text = MD.editor.value;
    const preview = U.$('#preview');
    const welcome = U.$('#welcome');
    const empty = text.trim() === '' && !state.currentFile;
    welcome.hidden = !empty;
    preview.hidden = empty;
    if (empty) { updateCounts(''); return; }
    try {
      await MD.md.render(text, preview, state.currentDir);
    } catch (e) {
      console.error(e);
    }
    // 给预览里的任务勾选框标上源码行号，点击时按行精确回写（与编辑器共用扫描规则）
    const taskLineNos = MD.md.taskLines(text);
    U.$$('input[data-task]', preview).forEach((cb, i) => {
      if (taskLineNos[i] != null) cb.dataset.line = taskLineNos[i];
    });
    if (seq !== renderSeq) return; // 已有更新请求
    updateCounts(text);
    MD.bus.emit('render:done');
    if (MD.settings.pagedPreview && MD.settings.paper !== 'flow') {
      U.$('#preview').hidden = true;
      // 必须带上 .markdown-body 包裹层：doc.css 里所有正文样式都以它为前缀，
      // 只传 innerHTML 会丢掉字号/主题色/表格边框等全部规则。
      await MD.actions.renderPaged('<article class="markdown-body">' + preview.innerHTML + '</article>');
    } else {
      U.$('#paged-output').hidden = true;
      U.$('#paged-output').innerHTML = '';
      MD.actions.clearPagedStyles();
    }
  }, 220);

  function updateCounts(text) {
    const words = U.countWords(text || '');
    U.$('#st-words').textContent = `${words} 字`;
    U.$('#st-chars').textContent = `${(text || '').length} 字符`;
    const min = Math.max(1, Math.round(words / 400));
    U.$('#st-reading').textContent = `约 ${min} 分钟`;
  }

  // doc:changed 每次按键都会同步跑一次全文字数统计，且与防抖后 renderNow 里那次
  // updateCounts 重复。用 rAF 把同一帧内的多次统计合并成一次，降低高频计算开销；
  // 渲染主链 renderNow 本身已是 U.debounce(220ms)，不重复叠加节流，
  // render:done 的触发语义与时机保持不变。
  let _countsRaf = 0;
  function scheduleUpdateCounts() {
    if (_countsRaf) return;
    _countsRaf = requestAnimationFrame(() => {
      _countsRaf = 0;
      updateCounts(MD.editor.value);
    });
  }

  function updateStatusBar() {
    const p = state.currentFile;
    const el = U.$('#st-path');
    el.textContent = p || '';
    el.title = p || '';
  }

  /* ══════════ 状态栏 / 窗口标题 ══════════ */
  let _lastTitle = '';
  const _titleTick = U.debounce(() => {
    const name = state.currentFile ? U.baseName(state.currentFile) : '未命名';
    const dirty = state.dirty ? ' ●' : '';
    const t = `MDeditor — ${name}${dirty}`;
    if (t === _lastTitle) return;
    _lastTitle = t;
    MD.fsx.setWindowTitle(t);
  }, 260);
  function updateWindowTitle() { _titleTick(); }

  /* ══════════ 视图切换 ══════════ */
  function setView(view) {
    if (!['source', 'split', 'preview', 'read'].includes(view)) view = 'split';
    MD.settings.view = view;
    document.documentElement.dataset.view = view;
    U.$$('#view-switch button').forEach((b) => b.classList.toggle('active', b.dataset.viewBtn === view));
    MD.saveSettings();
    setTimeout(() => MD.editor.cm && MD.editor.cm.refresh(), 50);
  }
  function toggleSidebar(force) {
    const on = force != null ? force : document.documentElement.dataset.sidebar !== 'off';
    document.documentElement.dataset.sidebar = on ? 'on' : 'off';
    MD.settings.sidebarOn = on;
    MD.saveSettings();
    setTimeout(() => MD.editor.cm && MD.editor.cm.refresh(), 50);
  }
  function setTypewriter(on) {
    MD.settings.typewriter = !!on;
    MD.editor.setTypewriter(MD.settings.typewriter);
    MD.saveSettings();
    MD.toast(MD.settings.typewriter ? '打字机滚动已开启' : '打字机滚动已关闭');
  }
  function changeEditorFontSize(d) {
    const next = MD.editor.changeFontSize(d);
    MD.toast(`编辑器字号：${next}px`);
  }

  /* ══════════ 同步滚动 ══════════ */
  function bindSyncScroll() {
    const cmEl = MD.editor.cm.getScrollerElement();
    const pv = U.$('#preview-scroll');
    let lock = null;
    let lockTimer = null;
    cmEl.addEventListener('scroll', () => {
      if (lock === 'preview') return;
      lock = 'editor';
      const ratio = cmEl.scrollTop / Math.max(1, cmEl.scrollHeight - cmEl.clientHeight);
      pv.scrollTop = ratio * (pv.scrollHeight - pv.clientHeight);
      clearTimeout(lockTimer);
      lockTimer = setTimeout(() => { if (lock === 'editor') lock = null; }, 80);
    });
    pv.addEventListener('scroll', U.throttle(() => {
      if (lock === 'editor') return;
      lock = 'preview';
      const ratio = pv.scrollTop / Math.max(1, pv.scrollHeight - pv.clientHeight);
      cmEl.scrollTop = ratio * (cmEl.scrollHeight - cmEl.clientHeight);
      setTimeout(() => { if (lock === 'preview') lock = null; }, 80);
    }, 50));
  }

  /* ══════════ 分隔条拖动 ══════════ */
  function bindSplitters() {
    const vsb = U.$('#vsplitter');
    const ws = U.$('#workspace');
    let dragging = false;
    vsb.addEventListener('mousedown', (e) => { dragging = true; e.preventDefault(); });
    window.addEventListener('mousemove', (e) => {
      if (!dragging) return;
      const rect = ws.getBoundingClientRect();
      const pct = U.clamp(((e.clientX - rect.left) / rect.width) * 100, 18, 82);
      document.documentElement.style.setProperty('--editor-ratio', pct.toFixed(1));
      MD.settings.editorRatio = pct;
    });
    window.addEventListener('mouseup', () => {
      if (dragging) { dragging = false; MD.saveSettings(); setTimeout(() => MD.editor.cm.refresh(), 30); }
    });

    const ssb = U.$('#sidebar-splitter');
    let dragS = false;
    ssb.addEventListener('mousedown', (e) => { dragS = true; e.preventDefault(); });
    window.addEventListener('mousemove', (e) => {
      if (!dragS) return;
      const w = U.clamp(e.clientX, 180, 480);
      document.documentElement.style.setProperty('--sidebar-w', w + 'px');
      MD.settings.sidebarWidth = w;
    });
    window.addEventListener('mouseup', () => {
      if (dragS) { dragS = false; MD.saveSettings(); setTimeout(() => MD.editor.cm.refresh(), 30); }
    });
  }

  /* ══════════ 菜单 ══════════ */
  function closeMenus() {
    U.$$('.menu.open').forEach((m) => m.classList.remove('open'));
    U.$$('.menu-pop').forEach((p) => { p.hidden = true; });
  }
  function openMenu(btn) {
    const name = btn.dataset.openMenu;
    const menu = btn.closest('.menu');
    if (menu.classList.contains('open')) { closeMenus(); return; }
    closeMenus();
    if (name === 'theme') MD.theme.renderThemeMenu();
    const pop = U.$(`[data-pop="${name}"]`);
    if (!pop) return;
    const rect = btn.getBoundingClientRect();
    // 只做一次搬家，之后靠 hidden 显隐（早先每次关闭都 remove() 会把静态菜单删掉，之后菜单就再也打不开）
    if (pop.parentElement !== document.body) document.body.appendChild(pop);
    pop.hidden = false;
    pop.style.top = rect.bottom + 6 + 'px';
    pop.style.left = Math.min(rect.left, window.innerWidth - pop.offsetWidth - 12) + 'px';
    menu.classList.add('open');
  }
  function bindMenus() {
    U.$$('.menu-btn').forEach((btn) => {
      btn.addEventListener('click', (e) => { e.stopPropagation(); openMenu(btn); });
    });
    document.addEventListener('click', (e) => {
      if (!e.target.closest('.menu-pop') && !e.target.closest('.menu-btn')) closeMenus();
    });

    // 主题菜单（事件委托，内容重建不影响）
    U.$('#theme-menu-pop').addEventListener('click', (e) => {
      const ui = e.target.closest('[data-ui-set]');
      const th = e.target.closest('[data-theme-set]');
      const act = e.target.closest('[data-act]');
      if (ui) { MD.settings.uiTheme = ui.dataset.uiSet; MD.theme.applyDoc(); MD.saveSettings(); closeMenus(); }
      else if (th) { MD.settings.docTheme = th.dataset.themeSet; MD.theme.applyDoc(); MD.saveSettings(); closeMenus(); MD.bus.emit('render:needed'); }
      else if (act && act.dataset.act === 'openThemeEditor') { closeMenus(); MD.theme.openThemeEditor(); }
    });
  }

  /* ══════════ [data-act] 全局委托 ══════════ */
  const ACT_MAP = {
    newFile: () => MD.actions.newFile(),
    openFile: () => MD.actions.openFileDialog(),
    openFolder: () => MD.actions.openFolderDialog(),
    importDoc: () => MD.actions.importAsMarkdown(),
    newFileInFolder: () => MD.actions.newFile(MD.panels.currentFolder),
    save: () => MD.actions.save(),
    saveAs: () => MD.actions.saveAs(),
    revealInExplorer: () => state.currentFile ? MD.fsx.reveal(state.currentFile) : MD.toast('请先打开文件'),
    closeDoc: () => MD.actions.closeDoc(),
    exportPDF: () => MD.actions.exportPDF(),
    printDoc: () => MD.actions.printDoc(),
    exportHTML: () => MD.actions.exportHTML(),
    exportWord: () => MD.actions.exportWord(),
    exportTXT: () => MD.actions.exportTXT(),
    exportTheme: () => MD.actions.exportTheme(),
    importTheme: () => MD.theme.importTheme(),
    viewSource: () => setView('source'),
    viewSplit: () => setView('split'),
    viewPreview: () => setView('preview'),
    viewRead: () => setView('read'),
    toggleSidebar: () => toggleSidebar(),
    toggleTypewriter: () => setTypewriter(!MD.settings.typewriter),
    toggleFullscreen: () => toggleFullscreen(),
    openThemeEditor: () => MD.theme.openThemeEditor(),
    clearRecent: () => { MD.panels.clearRecent(); MD.toast('已清空最近打开记录'); },
    closeSave: () => finishClose('save'),
    closeDiscard: () => finishClose('discard'),
    closeCancel: () => finishClose('cancel'),
  };
  function closeModal(id) {
    const el = id && U.$('#' + id);
    if (el) el.hidden = true;
    if (id === 'theme-modal') MD.theme.clearDraftPreview();
    // 退出确认被取消：解除状态锁，否则之后点 ✕ 会被当成重复请求忽略
    if (id === 'close-modal') closing = false;
  }
  function bindActions() {
    document.addEventListener('click', (e) => {
      const el = e.target.closest('[data-act]');
      if (el && ACT_MAP[el.dataset.act]) {
        e.preventDefault();
        ACT_MAP[el.dataset.act]();
        closeMenus();
      }
    });
    // 弹窗关闭（✕ / 点遮罩 / Esc 统一走这里，主题编辑器要清掉未保存的预览草稿）
    U.$$('[data-close-panel]').forEach((btn) => {
      btn.addEventListener('click', () => closeModal(btn.dataset.closePanel));
    });
    U.$$('.modal-mask').forEach((mask) => {
      mask.addEventListener('mousedown', (e) => { if (e.target === mask) closeModal(mask.id); });
    });
    U.$('#btn-layout').addEventListener('click', () => {
      const panel = U.$('#layout-panel');
      panel.hidden = !panel.hidden;
      if (!panel.hidden) MD.theme.renderLayoutPanel();
    });
    U.$('#btn-settings').addEventListener('click', () => {
      MD.theme.renderSettings();
      U.$('#settings-modal').hidden = false;
    });
    U.$('#st-theme').addEventListener('click', (e) => {
      // 阻止冒泡：document 上的「点空白关菜单」监听器会把刚打开的主题菜单立刻关掉
      e.stopPropagation();
      openMenu(U.$('#theme-menu-btn'));
    });
    U.$('#st-path').addEventListener('click', () => { if (state.currentFile) MD.fsx.reveal(state.currentFile); });
  }

  /* ══════════ 视图切换按钮（顶栏分段控件） ══════════ */
  function bindViewSwitch() {
    U.$$('#view-switch button').forEach((btn) => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        const v = btn.dataset.viewBtn;
        setView(v);
        // 从阅读/预览切回可编辑视图时，把光标交还编辑器，省一次点击
        if (v === 'split' || v === 'source') setTimeout(() => MD.editor.focus(), 60);
      });
    });
  }

  /* ══════════ 格式工具栏 ══════════ */
  function bindFmt() {
    U.$$('#fmt-toolbar [data-fmt]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const f = btn.dataset.fmt;
        const a = MD.editor.actions;
        switch (f) {
          case 'bold': a.bold(); break;
          case 'italic': a.italic(); break;
          case 'h': cycleHeading(); break;
          case 'quote': a.quote(); break;
          case 'ul': a.ul(); break;
          case 'ol': a.ol(); break;
          case 'table': a.table(); break;
          case 'link': a.link(); break;
          case 'image': pickAndInsertImage(); break;
          case 'code': a.codeBlock(); break;
          case 'task': a.task(); break;
          case 'hr': a.hr(); break;
        }
      });
    });
  }
  function cycleHeading() {
    const cm = MD.editor.cm;
    const line = cm.getCursor().line;
    const m = /^(#{1,6})\s/.exec(cm.getLine(line) || '');
    const cur = m ? m[1].length : 0;
    MD.editor.actions.heading(cur >= 3 ? 0 : cur + 1);
  }
  /** 判断 p 是否位于 dir 目录内：目录末尾补分隔符、Windows 大小写不敏感、必须严格长于前缀 */
  function isUnderDir(p, dir) {
    if (!p || !dir) return false;
    const sep = MD.fsx.sep || '/';
    const prefix = /[\\/]$/.test(dir) ? dir : dir + sep;
    if (p.length <= prefix.length) return false;
    const head = p.slice(0, prefix.length);
    return sep === '\\' ? head.toLowerCase() === prefix.toLowerCase() : head === prefix;
  }
  async function pickAndInsertImage() {
    const p = await MD.fsx.pickFile([
      { name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'] },
    ]);
    if (!p) return;
    const name = U.baseName(p);
    // 若图片与文档同目录，用相对路径；否则内嵌
    if (isUnderDir(p, state.currentDir)) {
      const rel = p.slice(state.currentDir.length).replace(/^[\\/]+/, '');
      MD.editor.actions.imageLink(rel.replace(/\\/g, '/'), name);
    } else {
      try {
        const url = await MD.fsx.readImageDataURL(p);
        MD.editor.actions.imageLink(url, name);
      } catch (e) {
        MD.editor.actions.imageLink(p, name);
      }
    }
  }

  /* ══════════ 快捷键 ══════════ */
  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else document.documentElement.requestFullscreen && document.documentElement.requestFullscreen();
  }
  function bindKeys() {
    document.addEventListener('keydown', (e) => {
      const mod = e.ctrlKey || e.metaKey;
      // 按住 Shift 时 e.key 会是大写（'S'/'O'），统一小写后再判断
      const k = (e.key || '').toLowerCase();
      // shift 分支必须排在普通 s/o/p 之前
      if (mod && e.shiftKey && k === 's') { e.preventDefault(); MD.actions.saveAs(); }
      else if (mod && e.shiftKey && k === 'o') { e.preventDefault(); MD.actions.openFolderDialog(); }
      else if (mod && e.altKey && k === 'o') { e.preventDefault(); MD.actions.importAsMarkdown(); }
      else if (mod && k === 's') { e.preventDefault(); MD.actions.save(); }
      else if (mod && k === 'n') { e.preventDefault(); MD.actions.newFile(); }
      else if (mod && k === 'o') { e.preventDefault(); MD.actions.openFileDialog(); }
      else if (mod && k === 'p') { e.preventDefault(); MD.actions.exportPDF(); }
      else if (mod && e.key === '\\') { e.preventDefault(); toggleSidebar(); }
      else if (e.altKey && k === '1') { e.preventDefault(); setView('source'); }
      else if (e.altKey && k === '2') { e.preventDefault(); setView('split'); }
      else if (e.altKey && k === '3') { e.preventDefault(); setView('preview'); }
      else if (e.altKey && k === '4') { e.preventDefault(); setView('read'); }
      else if (e.key === 'F11') { e.preventDefault(); toggleFullscreen(); }
      else if (e.key === 'Escape') {
        closeMenus();
        const openModal = U.$$('.modal-mask').some((m) => !m.hidden);
        U.$$('.modal-mask').forEach((m) => { if (!m.hidden) closeModal(m.id); });
        U.$('#layout-panel').hidden = true;
        // 阅读模式没有任何编辑入口时，Esc 就是「退出阅读」这条退路
        if (!openModal && document.documentElement.dataset.view === 'read') setView('split');
      }
    });
  }

  /* ══════════ 预览交互（链接 / 任务勾选） ══════════ */
  function bindPreview() {
    U.$('#preview').addEventListener('click', (e) => {
      // 外部链接
      const a = e.target.closest('a');
      if (a) {
        e.preventDefault();
        const href = a.getAttribute('href') || '';
        if (a.dataset.external) {
          MD.fsx.openExternal(href);
        } else if (href.startsWith('#')) {
          const id = decodeURIComponent(href.slice(1));
          const el = document.getElementById(id);
          let t = el && U.$('#preview').contains(el) ? el : null;
          if (!t) {
            // marked 生成的 heading id 通常是「小写、空格换 -、去标点」，
            // 这里在 #preview 内遍历 [id]，按大小写/字符归一化兜底比对（保留中文）
            const norm = (s) => String(s || '').toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, '');
            const wanted = norm(id);
            t = U.$$('[id]', U.$('#preview')).find((n) => norm(n.id) === wanted) || null;
          }
          if (t) t.scrollIntoView({ behavior: 'smooth', block: 'start' });
        } else if (a.dataset.mdLink) {
          // 相对链接 → 打开同级 md 文件
          const target = decodeURIComponent(href.split('#')[0]);
          if (U.isMarkdown(target) && state.currentDir) {
            MD.actions.openFile(U.joinPath(state.currentDir, target.replace(/\//g, '\\')));
          } else if (/^https?:/i.test(target)) {
            MD.fsx.openExternal(target);
          }
        }
        return;
      }
      // 任务列表勾选
      const cb = e.target.closest('input[data-task]');
      if (cb) {
        // 渲染时已标上源码行号，优先按行回写；没有 data-line 时回退按序号
        const line = cb.dataset.line;
        if (line != null && line !== '') {
          MD.editor.setTaskByLine(parseInt(line, 10), cb.checked);
        } else {
          const boxes = U.$$('input[data-task]', U.$('#preview'));
          const idx = boxes.indexOf(cb);
          MD.editor.setTaskByIndex(idx, cb.checked);
        }
        return;
      }
      // 点击预览跳到源码对应位置（分栏时）
      if (document.documentElement.dataset.view === 'split' && e.altKey) {
        // Alt+点击 → 大致定位
      }
    });
  }

  /* ══════════ 拖放文件到窗口打开 ══════════ */
  const IMG_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'];
  let dropHintTimer = 0;

  function showDropHint(on) {
    const el = U.$('#drop-hint');
    if (!el) return;
    clearTimeout(dropHintTimer);
    if (on) {
      el.hidden = false;
    } else {
      // 拖动过程中 leave / enter 会频繁抖动，延迟关掉避免闪
      dropHintTimer = setTimeout(() => { el.hidden = true; }, 80);
    }
  }

  /** 处理拖入的文件路径（桌面版：拿到的是真实路径） */
  async function openDroppedPaths(paths) {
    const list = (paths || []).filter(Boolean);
    if (!list.length) return;
    if (list.length > 1) MD.toast(`拖入了 ${list.length} 个文件，打开第一个`);
    const first = list[0];
    const ext = U.ext(first);

    // 图片 → 插入到当前文档
    if (IMG_EXTS.includes(ext)) {
      if (!state.currentFile) { MD.toast('请先打开或新建一篇文档，再拖入图片'); return; }
      try {
        const url = await MD.fsx.readImageDataURL(first);
        handleDroppedImage(url, U.baseName(first));
        MD.toast('已插入图片 ' + U.baseName(first));
      } catch (err) { MD.toast('读取图片失败：' + (err.message || err)); }
      return;
    }
    if (U.isMarkdown(first)) { await MD.actions.openFile(first); return; }
    if (ext === 'txt') { await MD.actions.openFile(first); return; }
    // PDF / Word / Excel / PPT / HTML / RTF / CSV → 转成 Markdown 再打开
    if (MD.actions.isConvertible(first)) { await MD.actions.importFile(first); return; }
    MD.toast('不支持的文件类型：' + U.baseName(first));
  }

  /** 桌面版：Tauri 2 的拖放事件是发给具体 webview 的，必须走 onDragDropEvent
   *  （Webview 的 listen 目标是 kind:Webview；WebviewWindow 的是 kind:WebviewWindow，
   *   全局 event.listen 是 Any —— 后两者都收不到拖放事件，所以优先用 getCurrentWebview） */
  async function bindNativeDragDrop() {
    const T = window.__TAURI__ || {};
    const candidates = [];
    if (T.webview && T.webview.getCurrentWebview) candidates.push(() => T.webview.getCurrentWebview());
    if (T.webviewWindow && T.webviewWindow.getCurrentWebviewWindow) candidates.push(() => T.webviewWindow.getCurrentWebviewWindow());

    let bound = false;
    for (const get of candidates) {
      if (bound) break;
      try {
        const wv = get();
        if (!wv || typeof wv.onDragDropEvent !== 'function') continue;
        await wv.onDragDropEvent((ev) => {
          const p = (ev && ev.payload) || {};
          if (p.type === 'enter' || p.type === 'over') showDropHint(true);
          else if (p.type === 'leave') showDropHint(false);
          else if (p.type === 'drop') { showDropHint(false); openDroppedPaths(p.paths); }
        });
        bound = true;
      } catch (e) { console.warn('onDragDropEvent 绑定失败', e); }
    }

    // 兜底：极老版本的全局广播
    if (!bound && T.event && T.event.listen) {
      try {
        await T.event.listen('tauri://drag-enter', () => showDropHint(true));
        await T.event.listen('tauri://drag-leave', () => showDropHint(false));
        await T.event.listen('tauri://drag-drop', (ev) => {
          showDropHint(false);
          openDroppedPaths((ev.payload && ev.payload.paths) || []);
        });
        bound = true;
      } catch (e) { console.warn('drag-drop 全局监听失败', e); }
    }
    if (!bound) console.warn('未找到可用的拖放接口，拖入文件不可用');
    // 留个痕迹，方便排查「拖进来没反应」
    try { localStorage.setItem('md.dragApi', bound ? 'bound' : 'none'); } catch (e) { /* ignore */ }
  }

  /** 浏览器版：整页接收 HTML5 拖放 */
  function bindBrowserDragDrop() {
    let depth = 0;
    const hasFiles = (e) => {
      const dt = e.dataTransfer;
      if (!dt) return false;
      const t = Array.from(dt.types || []);
      return t.includes('Files');
    };
    window.addEventListener('dragenter', (e) => {
      if (!hasFiles(e)) return;
      depth++;
      showDropHint(true);
    });
    window.addEventListener('dragover', (e) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      showDropHint(true);
    });
    window.addEventListener('dragleave', (e) => {
      if (!hasFiles(e)) return;
      depth = Math.max(0, depth - 1);
      if (depth === 0) showDropHint(false);
    });
    window.addEventListener('drop', (e) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      showDropHint(false);
      if (e.target.closest && e.target.closest('.CodeMirror')) return;
      MD.actions.handleDrop(e.dataTransfer);
    });
  }

  /* ══════════ Tauri 额外能力 / 启动文件 ══════════ */
  async function bindTauriExtras() {
    if (!MD.fsx.isApp) { bindBrowserDragDrop(); return; }
    await bindNativeDragDrop();
  }
  function handleDroppedImage(dataURL, name) {
    // 与粘贴图片一致的落盘逻辑
    MD.actions.handleImageDataURL(dataURL, name);
  }

  /* ══════════ 自动保存 ══════════ */
  let _autoSaveFailed = false;
  const autoSave = U.debounce(() => {
    if (MD.settings.autoSave && state.currentFile && state.dirty) {
      const gen = MD.editor.cm && MD.editor.cm.changeGeneration ? MD.editor.cm.changeGeneration() : null;
      MD.fsx.writeFile(state.currentFile, MD.editor.value)
        .then(() => {
          _autoSaveFailed = false;
          const cm = MD.editor.cm;
          // 写盘期间又改过：磁盘是旧快照，不清脏，留给下一次自动保存补写
          if (!gen || (cm && cm.isClean && cm.isClean(gen))) {
            MD.bus.emit('doc:saved');
          } else {
            MD.toast('保存期间文档又有修改，已保留未保存状态');
          }
        })
        .catch((e) => {
          // 不要静默失败：文件只读/被占用时用户必须知道内容没写进磁盘
          if (!_autoSaveFailed) {
            _autoSaveFailed = true;
            MD.toast('自动保存失败：' + (e && e.message ? e.message : e));
          }
        });
    }
  }, 1500);

  /* ══════════ 外部修改检测（低频轮询，仅桌面版真实文件） ══════════
     维护「基准 mtime」：打开 / 另存为 / 保存（含自动保存）成功后重新取样，
     否则下一次轮询会把「自己刚写的盘」误判成外部修改。 */
  const EXTERNAL_POLL_MS = 2000;
  let watchBasePath = null;       // 基准对应的文件路径
  let watchBaseMtime = null;      // 基准 mtime（Unix 毫秒）；null = 暂不可轮询
  let watchNeedsBaseline = true;  // 打开/保存后先置 true，基准重建完成前轮询不判定
  let watchSeq = 0;               // 代次：丢弃过期的异步 mtime 结果
  let watchBusy = false;          // 避免一次检查未完成时重叠

  function refreshWatchBaseline() {
    watchNeedsBaseline = true;
    const seq = ++watchSeq;
    const path = state.currentFile;
    if (!path || !MD.fsx.isApp) {
      watchBasePath = path || null;
      watchBaseMtime = null;
      return;
    }
    MD.fsx.fileMtime(path).then((m) => {
      if (seq !== watchSeq) return;   // 期间又打开/保存过，丢弃旧结果
      watchBasePath = path;
      watchBaseMtime = (m == null) ? null : m;
      watchNeedsBaseline = false;
    }).catch(() => { /* fileMtime 已吞异常，这里仅兜底 */ });
  }

  /** 用磁盘内容替换编辑器。MD.editor.value 的 setter 已 clearHistory + markClean
      并抑制 doc:changed；再借 doc:saved（现有事件）把 dirty 置 false 并重建基准。 */
  async function reloadFromDisk(path) {
    try {
      const text = await MD.fsx.readFile(path);
      MD.editor.value = text;
      MD.bus.emit('doc:saved');
      MD.app.renderPreview();
      MD.toast('文件已在外部更新，已重新加载');
    } catch (e) {
      MD.toast('重新加载失败：' + (e && e.message ? e.message : e));
    }
  }

  async function checkExternalChange() {
    if (document.hidden || watchBusy) return;
    const path = state.currentFile;
    if (!path || !MD.fsx.isApp) return;
    // 基准重建中、或刚切换文档基准还没跟上：跳过，不做「变化」判定
    if (watchNeedsBaseline || watchBaseMtime == null || path !== watchBasePath) return;
    watchBusy = true;
    try {
      const m = await MD.fsx.fileMtime(path);
      if (m == null || m === watchBaseMtime) return;
      // 先把基准对齐到本次观测值：无论重载成功 / 用户取消 / 读取失败，
      // 都不会对同一次外部修改反复弹窗或反复重载
      watchBaseMtime = m;
      if (state.dirty && !confirm('文件已在外部被修改，是否放弃本地未保存修改并重新加载？')) {
        return;                       // 取消：保留本地未保存修改，基准已更新
      }
      await reloadFromDisk(path);
    } finally {
      watchBusy = false;
    }
  }

  /** 单一递归 setTimeout 链：全程只启动一次，切换文档不会叠加旧计时器 */
  function startExternalWatch() {
    const tick = async () => {
      try { await checkExternalChange(); }
      catch (e) { console.warn('外部修改检测异常', e); }  // 别让轮询把异常抛到全局
      setTimeout(tick, EXTERNAL_POLL_MS);
    };
    setTimeout(tick, EXTERNAL_POLL_MS);
  }

  /* ══════════ 关闭流程 ══════════ */
  // Rust 侧拦截 CloseRequested 后广播 app:close-requested，前端只做决策。
  // 绝不再用 window.onCloseRequested：那条链路要求前端调用 window.destroy()，
  // 而默认 capabilities 不含 core:window:allow-destroy，会导致窗口彻底关不掉。
  let closing = false;

  function bindCloseFlow() {
    const T = window.__TAURI__ || {};
    if (T.event && typeof T.event.listen === 'function') {
      T.event.listen('app:close-requested', () => { onCloseRequested(); })
        .catch((e) => console.warn('关闭事件订阅失败', e));
    } else {
      // 浏览器模式兜底（桌面版走原生拦截）
      window.addEventListener('beforeunload', (e) => {
        if (state.dirty) { e.preventDefault(); e.returnValue = ''; }
      });
    }
  }

  function onCloseRequested() {
    if (closing) return;
    if (!state.dirty) { MD.fsx.quit(); return; }   // 无未保存修改：直接退出
    closing = true;
    const mask = U.$('#close-modal');
    if (mask) mask.hidden = false;
    else MD.fsx.quit();   // 弹窗缺失时不让用户卡住
  }

  async function finishClose(action) {
    if (action === 'cancel') {
      closing = false;
      closeModal('close-modal');
      return;
    }
    if (action === 'save') {
      try {
        await MD.actions.save();          // 取消「另存为」会走到这里，dirty 保持 true
      } catch (e) {
        MD.toast('保存失败：' + ((e && e.message) || e));
        return;                           // 留在窗口，绝不带着未保存内容退出
      }
      if (state.dirty) { MD.toast('尚未保存成功，已取消退出'); return; }
    }
    await MD.fsx.quit();
  }

  /* ══════════ 启动 ══════════ */
  async function boot() {
    MD.loadSettings();
    const s = MD.settings;

    // 恢复 UI 状态
    document.documentElement.style.setProperty('--sidebar-w', s.sidebarWidth + 'px');
    document.documentElement.style.setProperty('--editor-ratio', s.editorRatio);
    MD.theme.applyDoc();
    MD.editor.init(U.$('#editor'));
    MD.editor.applyFontSize(s.fontSize);
    if (MD.zoom) MD.zoom.init(s.zoom);
    MD.editor.setTypewriter(s.typewriter);
    MD.panels.init();
    bindMenus();
    bindActions();
    bindViewSwitch();
    bindFmt();
    bindKeys();
    bindPreview();
    bindTauriExtras().catch((e) => console.warn('拖放初始化失败', e));
    bindSplitters();
    bindSyncScroll();
    // 阅读模式是「临时看一眼」的状态，不该被带到下次启动——否则一打开就只剩预览，
    // 看起来就像「没有编辑功能」
    setView(s.view === 'read' ? 'split' : s.view);
    toggleSidebar(s.sidebarOn);

    // 状态联动
    MD.bus.on('doc:changed', () => {
      renderNow();
      scheduleUpdateCounts();
      updateStatusBar();
      autoSave();
    });
    MD.bus.on('cursor:moved', (pos) => {
      U.$('#st-cursor').textContent = `行 ${pos.line + 1}，列 ${pos.ch + 1}`;
    });
    MD.bus.on('doc:opened', () => { updateStatusBar(); updateWindowTitle(); refreshWatchBaseline(); });
    MD.bus.on('doc:saved', () => { updateWindowTitle(); refreshWatchBaseline(); });
    // 文件树里删掉当前正在编辑的文件：必须清空当前文件引用，
    // 否则 1.5 秒后的自动保存会把已删除的文件又写回磁盘（"文件复活"）。
    MD.bus.on('doc:close-request', () => { MD.actions.closeDocForce(); });
    MD.bus.on('doc:renamed', () => { updateStatusBar(); updateWindowTitle(); refreshWatchBaseline(); });
    MD.bus.on('theme:applied', () => {
      U.$('#st-theme').textContent = (MD.theme.getTheme(MD.settings.docTheme).name || '主题');
    });
    MD.bus.on('render:needed', renderNow);
    MD.bus.emit('theme:applied');
    startExternalWatch();

    bindCloseFlow();

    // 启动文件（文件关联双击打开）
    const startup = await MD.fsx.startupFile();
    if (startup && U.isMarkdown(startup)) {
      await MD.actions.openFile(startup);
    } else if (startup) {
      // 非 Markdown 的启动参数不再静默忽略：转成 .md 后打开
      await MD.actions.importFile(startup);
    } else {
      // 恢复上次文件夹
      if (s.lastFolder) {
        try {
          if (await MD.fsx.exists(s.lastFolder)) MD.panels.setFolder(s.lastFolder);
        } catch (e) { /* ignore */ }
      }
      renderNow();
    }

    // 无文档时预览区显示欢迎页
    if (!MD.editor.value.trim()) {
      U.$('#welcome').hidden = false;
      U.$('#preview').hidden = true;
    }

    // --pdf-out <路径>：渲染完成后直接导出 PDF 并退出（自动化 / 自测用）
    maybeAutoExportPdf();
  }

  /** 等一次渲染真正完成；Mermaid 图多时固定 900ms 往往还没画完 */
  function waitForRenderDone(maxMs) {
    return new Promise((resolve) => {
      let settled = false;
      let timer = null;
      function finish() {
        if (settled) return;
        settled = true;
        MD.bus.off('render:done', onDone);
        if (timer) clearTimeout(timer);
        resolve();
      }
      function onDone() {
        // render:done 之后 Mermaid / KaTeX 仍可能异步收尾，再让两帧
        requestAnimationFrame(() => requestAnimationFrame(finish));
      }
      MD.bus.on('render:done', onDone);
      timer = setTimeout(finish, maxMs);
    });
  }

  async function maybeAutoExportPdf() {
    if (!MD.fsx.isApp) return;
    let target = null;
    try { target = await MD.fsx.pdfTarget(); } catch (e) { return; }
    if (!target) return;
    let code = 1;
    try {
      // 等字体、KaTeX、Mermaid 画完，否则 PDF 里公式是空白；最多等 8 秒
      await waitForRenderDone(8000);
      const ok = await MD.actions.exportPDFTo(target, { silent: true });
      code = ok ? 0 : 1;
      console.log(ok ? 'pdf-export-ok ' + target : 'pdf-export-failed ' + target);
    } catch (e) {
      console.error('自动导出 PDF 失败', e);
    } finally {
      // 批处理脚本靠退出码判断是否成功，不能恒为 0
      setTimeout(() => { MD.fsx.quit(code); }, 500);
    }
  }

  MD.app = {
    boot, renderPreview: renderNow, setView, toggleSidebar, setTypewriter,
    changeEditorFontSize, updateWindowTitle, showHelp: () => MD.theme.showHelp(),
    // 拖放相关（同时便于调试/测试直接调用）
    openDroppedPaths, showDropHint,
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
