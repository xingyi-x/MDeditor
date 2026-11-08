/* ══════════════ core.js：工具函数 + 设置持久化 ══════════════ */
'use strict';
window.MD = window.MD || {};

// paged.js 关闭自动执行，改为手动触发。必须在动态加载 paged.polyfill 之前设置。
// 原来写在 index.html 的内联 <script> 里，为了启用严格 CSP 移到了这里。
window.PagedConfig = { auto: false };

MD.util = {
  $(sel, root) { return (root || document).querySelector(sel); },
  $$(sel, root) { return Array.from((root || document).querySelectorAll(sel)); },
  esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  },
  debounce(fn, ms) {
    let t = null;
    const wrapped = function (...a) { clearTimeout(t); t = setTimeout(() => fn.apply(this, a), ms); };
    wrapped.cancel = () => clearTimeout(t);
    return wrapped;
  },
  throttle(fn, ms) {
    let last = 0, timer = null;
    return function (...a) {
      const now = Date.now();
      const rest = ms - (now - last);
      if (rest <= 0) { last = now; fn.apply(this, a); }
      else if (!timer) timer = setTimeout(() => { last = Date.now(); timer = null; fn.apply(this, a); }, rest);
    };
  },
  baseName(p) { return String(p || '').replace(/[\\/]+$/, '').split(/[\\/]/).pop(); },
  dirName(p) {
    const s = String(p || '').replace(/[\\/]+$/, '');
    const i = Math.max(s.lastIndexOf('\\'), s.lastIndexOf('/'));
    if (i < 0) return '';
    // 盘符根目录要保留结尾分隔符：D:\a.md → D:\（不能返回 "D:"）
    if (i === 2 && s[1] === ':') return s.slice(0, 3);
    return i > 0 ? s.slice(0, i) : '';
  },
  joinPath(dir, name) {
    if (!dir) return name;
    return dir.replace(/[\\/]+$/, '') + (dir.includes('/') && !dir.includes('\\') ? '/' : '\\') + name;
  },
  ext(p) {
    const b = MD.util.baseName(p);
    const i = b.lastIndexOf('.');
    return i > 0 ? b.slice(i + 1).toLowerCase() : '';
  },
  isMarkdown(p) {
    const e = MD.util.ext(p);
    return e === 'md' || e === 'markdown';
  },
  bytes(n) {
    if (n < 1024) return n + ' B';
    if (n < 1048576) return (n / 1024).toFixed(1) + ' KB';
    return (n / 1048576).toFixed(1) + ' MB';
  },
  time(ms) {
    const d = new Date(ms);
    const pad = (x) => String(x).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  },
  // 中英混合字数：CJK 字符每字算 1，连续西文单词算 1
  countWords(text) {
    const cjk = (text.match(/[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff\u3040-\u30ff\uac00-\ud7af]/g) || []).length;
    const latin = (text.replace(/[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff\u3040-\u30ff\uac00-\ud7af]/g, ' ').match(/[A-Za-z0-9_'-]+/g) || []).length;
    return cjk + latin;
  },
  uid() { return Date.now().toString(36) + Math.random().toString(36).slice(2, 7); },
  clamp(x, a, b) { return Math.max(a, Math.min(b, x)); },
  // 字符串尺寸（用于表格列宽提示等）
  plainLen(s) { return String(s || '').replace(/\s+/g, ' ').trim().length; },
};

/* ── Toast 提示 ── */
let _toastTimer = null;
MD.toast = function (msg, ms = 2200) {
  const el = MD.util.$('#toast');
  el.textContent = msg;
  el.hidden = false;
  clearTimeout(_toastTimer);
  _toastTimer = setTimeout(() => { el.hidden = true; }, ms);
};

/* ── 简易事件总线 ── */
MD.bus = {
  map: {},
  on(ev, fn) { (this.map[ev] = this.map[ev] || []).push(fn); return fn; },
  off(ev, fn) { const a = this.map[ev]; if (a) { const i = a.indexOf(fn); if (i >= 0) a.splice(i, 1); } },
  emit(ev, data) { (this.map[ev] || []).slice().forEach((fn) => { try { fn(data); } catch (e) { console.error(ev, e); } }); },
};

/* ── 设置存储 ── */
const SETTINGS_KEY = 'mdeditor.settings.v1';

MD.defaults = {
  // 界面
  uiTheme: 'light',            // light | dark
  sidebarOn: true,
  sidebarWidth: 250,
  editorRatio: 50,
  view: 'split',               // source | split | preview | read
  typewriter: false,
  fontSize: 14.5,              // 编辑器字号
  zoom: 100,                   // 画面缩放百分比（Ctrl+滚轮）
  // 主题
  docTheme: 'qingjie',         // 文档主题 id
  customThemes: {},            // id -> {name, vars, extraCSS, base}
  // 排版
  paper: 'flow',               // flow | A4 | A5 | B5 | letter | 16k
  margins: { top: 24, bottom: 24, left: 20, right: 20 }, // mm（纸张模式）
  flowPadding: 56,             // px（流式模式四周留白）
  fontSizeDoc: 16,             // 文档正文字号 px
  lineHeightDoc: '',           // '' = 跟随主题；数值 = 用户指定
  paraGapDoc: 1,               // em
  indentDoc: '',               // '' = 跟随主题；数值 em
  alignDoc: '',                // '' = 跟随主题；left | justify
  fontBody: '',
  fontHeading: '',
  fontMono: '',
  maxWidthDoc: 860,
  headingScale: 1,
  accentColor: '',
  pagedPreview: false,
  // 文档
  autoSave: true,
  pasteImageToLocal: true,
  // 状态
  lastFolder: '',
  recentFiles: [],             // [{path,name,time}]
  restoreTabs: true,           // 启动时恢复上次打开的标签页
  openDocs: [],                // 上次打开的文件路径（仅路径，不含内容）
  activeDoc: '',               // 上次的活动标签
  settingsVersion: 2,
};

MD.settings = null;

MD.loadSettings = function () {
  let rawObj = {};
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    rawObj = raw ? JSON.parse(raw) : {};
    MD.settings = Object.assign({}, MD.defaults, rawObj);
  } catch (e) {
    console.warn('settings load failed', e);
    MD.settings = Object.assign({}, MD.defaults);
  }
  // 深合并 margins
  MD.settings.margins = Object.assign({}, MD.defaults.margins, MD.settings.margins || {});
  // 版本迁移：v1 的排版默认值偏挤，且用具体数值（1.8 / 0 / left）压住了主题自带的排版特征
  // （例如「仿 Word」的 2em 首行缩进 + 两端对齐换主题后不生效）。v2 起改为「跟随主题」语义。
  // 只刷排版这一组，主题、界面、编辑习惯等用户偏好保持不动。
  const ver = Number(rawObj.settingsVersion) || 1;
  if (ver < 2) {
    const d = MD.defaults;
    MD.settings.margins = Object.assign({}, d.margins);
    Object.assign(MD.settings, {
      settingsVersion: 2,
      flowPadding: d.flowPadding,
      lineHeightDoc: d.lineHeightDoc,
      paraGapDoc: d.paraGapDoc,
      maxWidthDoc: d.maxWidthDoc,
      indentDoc: d.indentDoc,
      alignDoc: d.alignDoc,
    });
  }
  return MD.settings;
};

let _saveSettings = null;
MD.saveSettings = function () {
  if (!_saveSettings) {
    _saveSettings = MD.util.debounce(() => {
      try { localStorage.setItem(SETTINGS_KEY, JSON.stringify(MD.settings)); } catch (e) { console.warn(e); }
    }, 400);
  }
  _saveSettings();
};
