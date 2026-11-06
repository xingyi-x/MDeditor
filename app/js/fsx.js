/* ══════════════ fsx.js：文件系统桥（Tauri 原生 / 浏览器回退） ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  const hasTauri = !!(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke);
  const hasFSA = !!(window.showOpenFilePicker || window.showDirectoryPicker);

  const fsx = {
    mode: hasTauri ? 'tauri' : 'browser',
    sep: hasTauri ? '\\' : '/',
    isApp: hasTauri,
  };

  // Tauri 的 invoke 失败时 reject 的是 Rust 返回的字符串（不是 Error），
  // 直接取 e.message 会得到 undefined，这里统一包成 Error。
  async function invoke(cmd, args) {
    try {
      return await window.__TAURI__.core.invoke(cmd, args || {});
    } catch (e) {
      const msg = typeof e === 'string' ? e : (e && e.message) || String(e);
      throw new Error(msg);
    }
  }

  /* ───────── Tauri 模式：直接映射到 Rust 命令 ───────── */
  // 打开文件时记录原始编码 / BOM / 换行风格，保存时按原样写回。
  // 不这么做的话，GBK 或 UTF-16 的文档一旦编辑就会被静默改成无 BOM UTF-8 + LF。
  const fileMeta = new Map();
  const tauriImpl = {
    sep: '\\',
    join: U.joinPath,
    async readFile(p) {
      const info = await invoke('read_text_file', { path: p });
      if (info && typeof info === 'object' && typeof info.text === 'string') {
        fileMeta.set(p, { encoding: info.encoding, bom: !!info.bom, eol: info.eol });
        return info.text;
      }
      return info; // 兼容只返回字符串的旧后端
    },
    async writeFile(p, content) {
      const m = fileMeta.get(p) || {};
      await invoke('write_text_file', {
        path: p,
        content,
        encoding: m.encoding || 'utf-8',
        bom: !!m.bom,
        eol: m.eol || '\n',
      });
    },
    /** 重命名 / 另存为之后把编码信息带到新路径 */
    inheritMeta(from, to) {
      if (fileMeta.has(from)) fileMeta.set(to, fileMeta.get(from));
    },
    async readImageDataURL(p) { return invoke('read_image_data_url', { path: p }); },
    async writeBinary(p, data) { await invoke('write_base64_file', { path: p, data }); },
    async listDir(p) {
      const items = await invoke('list_dir', { path: p });
      return items.map((i) => ({ name: i.name, path: i.path, isDir: i.is_dir, ext: i.ext, size: i.size, mtimeMs: i.modified_ms }));
    },
    async stat(p) {
      const i = await invoke('path_stat', { path: p });
      return i ? { name: i.name, path: i.path, isDir: i.is_dir, ext: i.ext, size: i.size, mtimeMs: i.modified_ms } : null;
    },
    async exists(p) { return invoke('path_exists', { path: p }); },
    /** 文件最后修改时间（Unix 毫秒）。Rust 命令约定：Ok(Some(u64)) → 数字、
     *  Ok(None) → null（文件不存在）、Err(String) → 抛错。这里统一吞掉异常返回 null，
     *  避免低频轮询把错误抛到全局。 */
    async fileMtime(p) {
      try {
        const v = await invoke('file_mtime', { path: p });
        return (typeof v === 'number' && Number.isFinite(v)) ? v : null;
      } catch (e) {
        return null;
      }
    },
    async mkdir(p) { await invoke('create_dir', { path: p }); },
    async remove(p) { await invoke('remove_path', { path: p }); },
    async rename(from, to) { await invoke('rename_path', { from, to }); },
    async findMarkdown(root, maxDepth) {
      const items = await invoke('find_markdown_files', { root, maxDepth: maxDepth || 6 });
      return items.map((i) => ({ name: i.name, path: i.path, isDir: false, ext: i.ext, size: i.size, mtimeMs: i.modified_ms }));
    },
    async search(root, query, caseSensitive) {
      return invoke('search_in_files', { root, query, caseSensitive: !!caseSensitive, maxHits: 300 });
    },
    async homeDir() { return invoke('home_dir', {}); },
    async reveal(p) { await invoke('reveal_in_explorer', { path: p }); },
    async openExternal(p) { await invoke('open_with_default_app', { path: p }); },
    async print() {
      try { await invoke('print_current_window', {}); }
      catch (e) { console.warn(e); window.print(); }
    },
    /** 直接写 PDF 文件（WebView2 PrintToPdf，无页眉页脚） */
    async exportPdf(path, opts) { return invoke('export_pdf', { path, opts }); },
    async pickFile(filters, defaultPath) {
      const opt = { multiple: false, filters: filters || [] };
      if (defaultPath) opt.defaultPath = defaultPath;
      const r = await window.__TAURI__.dialog.open(opt);
      return Array.isArray(r) ? r[0] : r;
    },
    async pickSaveFile(defaultName, filters) {
      return window.__TAURI__.dialog.save({ defaultPath: defaultName, filters: filters || [] });
    },
    async pickFolder() {
      const r = await window.__TAURI__.dialog.open({ directory: true, multiple: false });
      return Array.isArray(r) ? r[0] : r;
    },
    /** 文档 → Markdown。返回 { path, warnings }（旧后端只返回路径字符串） */
    async convertToMd(input, output, opt) {
      const o = opt || {};
      return invoke('convert_to_md', {
        input,
        output,
        images: o.images !== false,
        frontMatter: !!o.frontMatter,
        overwrite: o.overwrite !== false,
      });
    },
    async canConvert(p) {
      try { return await invoke('can_convert', { path: p }); } catch (e) { return false; }
    },
    async startupFile() {
      try { return await invoke('get_startup_file', {}); } catch (e) { return null; }
    },
    /** --pdf-out 目标路径（自动化导出用，普通启动为 null） */
    async pdfTarget() {
      try { return await invoke('get_pdf_target', {}); } catch (e) { return null; }
    },
    async quit(code) {
      // --pdf-out 自动化导出用退出码回报成功/失败
      const args = typeof code === 'number' ? { code } : {};
      try { await invoke('quit_app', args); } catch (e) { window.close(); }
    },
    async appVersion() {
      try { return await invoke('get_app_version', {}); } catch (e) { return ''; }
    },
    async setWindowTitle(t) {
      try { await invoke('set_window_title', { title: t }); } catch (e) { document.title = t; }
    },
    // 文件关联（把 .md / .markdown 关联到本程序）
    async registerMdAssoc() {
      return invoke('register_md_assoc', {});
    },
    async mdAssocStatus() {
      try { return await invoke('md_assoc_status', {}); } catch (e) { return false; }
    },
    async openDefaultAppsSettings() {
      try { await invoke('open_default_apps_settings', {}); } catch (e) { /* ignore */ }
    },
  };

  /* ───────── 浏览器模式：File System Access API ───────── */
  const bState = { root: null, handles: new Map(), contents: new Map() };

  function normParts(p) {
    return String(p || '').replace(/\\/g, '/').split('/').filter(Boolean);
  }
  async function resolveHandle(p, opt) {
    if (!bState.root && bState.handles.size === 0) throw new Error('浏览器模式下请先通过“打开文件/文件夹”选择位置');
    const parts = normParts(p);
    if (parts.length === 1 && bState.handles.has('/' + parts[0]) && !opt) {
      return bState.handles.get('/' + parts[0]);
    }
    if (!bState.root) throw new Error('找不到文件：' + p);
    let dir = bState.root;
    for (let i = 0; i < parts.length - 1; i++) dir = await dir.getDirectoryHandle(parts[i], { create: false });
    return dir.getFileHandle(parts[parts.length - 1], opt || {});
  }
  async function resolveDirForWrite(p) {
    const parts = normParts(p);
    if (!bState.root) throw new Error('浏览器模式下请先“打开文件夹”');
    let dir = bState.root;
    for (let i = 0; i < parts.length - 1; i++) dir = await dir.getDirectoryHandle(parts[i], { create: true });
    return { dir, name: parts[parts.length - 1] };
  }
  async function dirHandleFor(dirHandle, relParts) {
    let d = dirHandle;
    for (const part of relParts) d = await d.getDirectoryHandle(part, { create: false });
    return d;
  }

  const browserImpl = {
    sep: '/',
    join(dir, name) { return (dir ? String(dir).replace(/\/+$/, '') : '') + '/' + name; },
    async readFile(p) {
      // 注意用 != null 判断：registerBrowserFile 只登记句柄、内容缓存写的是 null，
      // 早先 has() 判断会把 null 当成缓存命中，拖进来的文件打开后是空白。
      const cached = bState.contents.get(p);
      if (cached != null) return cached;
      const h = await resolveHandle(p);
      const text = await (await h.getFile()).text();
      return text;
    },
    async writeFile(p, content) {
      bState.contents.set(p, content);
      const { dir, name } = await resolveDirForWrite(p);
      const fh = await dir.getFileHandle(name, { create: true });
      const w = await fh.createWritable();
      await w.write(content);
      await w.close();
    },
    async readImageDataURL(p) {
      const h = await resolveHandle(p);
      const blob = await (await h.getFile()).slice();
      return await new Promise((res, rej) => {
        const fr = new FileReader();
        fr.onload = () => res(fr.result);
        fr.onerror = rej;
        fr.readAsDataURL(blob);
      });
    },
    async writeBinary(p, data) {
      const blob = data.startsWith('data:') ? await (await fetch(data)).blob() : null;
      const { dir, name } = await resolveDirForWrite(p);
      const fh = await dir.getFileHandle(name, { create: true });
      const w = await fh.createWritable();
      await w.write(blob || new Blob([data]));
      await w.close();
    },
    async listDir(p) {
      const parts = normParts(p);
      const dh = parts.length ? await dirHandleFor(bState.root, parts) : bState.root;
      const out = [];
      for await (const [name, h] of dh.entries()) {
        const full = (p === '/' ? '' : p) + '/' + name;
        if (h.kind === 'directory') out.push({ name, path: full, isDir: true, ext: '', size: 0, mtimeMs: 0 });
        else {
          const f = await h.getFile();
          out.push({ name, path: full, isDir: false, ext: U.ext(name), size: f.size, mtimeMs: f.lastModified });
        }
      }
      out.sort((a, b) => (b.isDir - a.isDir) || a.name.localeCompare(b.name, 'zh'));
      return out;
    },
    async stat(p) {
      try {
        const h = await resolveHandle(p);
        const f = await h.getFile();
        return { name: U.baseName(p), path: p, isDir: h.kind === 'directory', ext: U.ext(p), size: f.size, mtimeMs: f.lastModified };
      } catch (e) { return null; }
    },
    async exists(p) { return (await this.stat(p)) != null; },
    /** 浏览器模式没有 file_mtime 命令，退回到 File System Access 的 lastModified（毫秒） */
    async fileMtime(p) {
      try {
        const st = await this.stat(p);
        return st && typeof st.mtimeMs === 'number' ? st.mtimeMs : null;
      } catch (e) { return null; }
    },
    async mkdir(p) { await resolveDirForWrite(p + '/x'); },
    async remove(p) {
      const parts = normParts(p);
      const parent = await dirHandleFor(bState.root, parts.slice(0, -1));
      try { await parent.removeEntry(parts[parts.length - 1], { recursive: true }); } catch (e) { /* ignore */ }
      bState.contents.delete(p);
    },
    async rename(from, to) {
      const text = await this.readFile(from);
      await this.writeFile(to, text);
      if (from !== to) await this.remove(from);
    },
    async findMarkdown(root) {
      const out = [];
      if (!bState.root) return out;
      async function walk(dh, prefix, depth) {
        if (depth > 6 || out.length > 2000) return;
        for await (const [name, h] of dh.entries()) {
          if (name.startsWith('.') || name === 'node_modules') continue;
          const full = prefix + '/' + name;
          if (h.kind === 'directory') await walk(h, full, depth + 1);
          else if (U.isMarkdown(name)) {
            const f = await h.getFile();
            out.push({ name, path: full, isDir: false, ext: U.ext(name), size: f.size, mtimeMs: f.lastModified });
          }
        }
      }
      await walk(bState.root, '', 0);
      return out;
    },
    async search(root, query, caseSensitive) {
      const files = await this.findMarkdown(root);
      const hits = [];
      for (const f of files) {
        if (hits.length >= 300) break;
        let text = '';
        try { text = await this.readFile(f.path); } catch (e) { continue; }
        const lines = text.split('\n');
        for (let i = 0; i < lines.length; i++) {
          const hay = caseSensitive ? lines[i] : lines[i].toLowerCase();
          const needle = caseSensitive ? query : query.toLowerCase();
          const col = hay.indexOf(needle);
          if (col >= 0) {
            hits.push({ path: f.path, name: f.name, line: i + 1, column: col + 1, text: lines[i].slice(0, 200) });
            if (hits.length >= 300) break;
          }
        }
      }
      return hits;
    },
    async homeDir() { return '/'; },
    async reveal() { MD.toast('浏览器模式不支持该操作'); },
    async openExternal(url) { window.open(url, '_blank'); },
    async print() { window.print(); },
    async exportPdf() { throw new Error('浏览器模式请用「打印」→ 另存为 PDF'); },
    async pickFile() {
      if (!window.showOpenFilePicker) { MD.toast('当前浏览器不支持文件选择器，请使用拖拽打开'); return null; }
      const [h] = await window.showOpenFilePicker({ types: [{ description: 'Markdown', accept: { 'text/markdown': ['.md', '.markdown'] } }] });
      const p = '/' + h.name;
      bState.handles.set(p, h);
      return p;
    },
    async pickSaveFile(defaultName) {
      if (!window.showSaveFilePicker) return null;
      const h = await window.showSaveFilePicker({ suggestedName: defaultName || 'untitled.md' });
      const p = '/' + h.name;
      bState.handles.set(p, h);
      return p;
    },
    async pickFolder() {
      if (!window.showDirectoryPicker) { MD.toast('当前浏览器不支持文件夹选择'); return null; }
      bState.root = await window.showDirectoryPicker();
      return '/';
    },
    async startupFile() { return null; },
    async pdfTarget() { return null; },
    async quit() { window.close(); },
    async appVersion() { return ''; },
    inheritMeta() {},
    async setWindowTitle(t) { document.title = t; },
    async registerMdAssoc() { throw new Error('仅桌面版支持'); },
    async mdAssocStatus() { return false; },
    async openDefaultAppsSettings() { MD.toast('仅桌面版支持'); },
  };

  // 注册拖拽打开的浏览器文件句柄
  MD.registerBrowserFile = function (file) {
    const p = '/' + file.name;
    bState.handles.set(p, { getFile: async () => file, kind: 'file', name: file.name });
    bState.contents.set(p, null);
    return p;
  };
  MD.setBrowserRoot = function (dirHandle) { bState.root = dirHandle; };

  Object.assign(fsx, hasTauri ? tauriImpl : browserImpl);
  MD.fsx = fsx;
})();
