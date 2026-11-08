/* ══════════════ theme.js：主题系统 + 排版（Word/PDF 式）设置 ══════════════ */
'use strict';

(function () {
  const U = MD.util;

  /* ── 字体选项 ── */
  const FONTS_BODY = [
    { v: '', label: '跟随主题' },
    { v: '"Microsoft YaHei", "PingFang SC", sans-serif', label: '微软雅黑' },
    { v: 'SimSun, "Songti SC", serif', label: '宋体' },
    { v: 'SimHei, "Heiti SC", sans-serif', label: '黑体' },
    { v: 'KaiTi, "Kaiti SC", STKaiti, serif', label: '楷体' },
    { v: 'FangSong, "Fangsong SC", STFangsong, serif', label: '仿宋' },
    { v: 'DengXian, sans-serif', label: '等线' },
    { v: 'Georgia, "Times New Roman", serif', label: 'Georgia（西文衬线）' },
    { v: 'Arial, Helvetica, sans-serif', label: 'Arial' },
  ];
  const FONTS_HEADING = FONTS_BODY.slice(1);
  const FONTS_MONO = [
    { v: '', label: '跟随主题' },
    { v: 'Consolas, "Microsoft YaHei", monospace', label: 'Consolas' },
    { v: '"Cascadia Code", Consolas, monospace', label: 'Cascadia Code' },
    { v: '"Courier New", monospace', label: 'Courier New' },
    { v: 'Menlo, Consolas, monospace', label: 'Menlo' },
  ];

  /* ── 内置文档主题 ── */
  const BUILTIN = [
    {
      id: 'qingjie', name: '清爽白', dark: false,
      vars: {
        '--doc-bg': '#ffffff', '--doc-fg': '#24292f', '--doc-muted': '#6b7280',
        '--doc-heading': '#111827', '--doc-link': '#2563eb', '--doc-strong': '#111827',
        '--doc-hr': '#d0d7de', '--doc-quote-bg': '#f6f8fa', '--doc-quote-bar': '#d0d7de',
        '--doc-code-bg': '#f3f4f6', '--doc-code-fg': '#24292f', '--doc-code-border': '#e5e7eb',
        '--doc-table-border': '#d0d7de', '--doc-table-head-bg': '#f6f8fa', '--doc-table-alt-bg': '#f9fafb',
        '--doc-mark-bg': 'rgba(255,212,0,0.4)',
        '--doc-font-body': '"Microsoft YaHei", "Segoe UI", sans-serif',
        '--doc-font-heading': '"Microsoft YaHei", "Segoe UI", sans-serif',
        '--doc-font-mono': 'Consolas, "Courier New", monospace',
      },
    },
    {
      id: 'nuanyang', name: '暖黄护眼', dark: false,
      vars: {
        '--doc-bg': '#f9f3e3', '--doc-fg': '#4a4234', '--doc-muted': '#8a7d68',
        '--doc-heading': '#3e3423', '--doc-link': '#9a6b1f', '--doc-strong': '#3e3423',
        '--doc-hr': '#e0d4b8', '--doc-quote-bg': '#f3ebd3', '--doc-quote-bar': '#c9b98f',
        '--doc-code-bg': '#f1e7cc', '--doc-code-fg': '#4a4234', '--doc-code-border': '#e0d4b8',
        '--doc-table-border': '#dccfa9', '--doc-table-head-bg': '#f1e7cc', '--doc-table-alt-bg': '#f6efdc',
        '--doc-mark-bg': 'rgba(240,180,60,0.35)',
      },
    },
    {
      id: 'mollyu', name: '豆沙绿', dark: false,
      vars: {
        '--doc-bg': '#cfe6d0', '--doc-fg': '#2e3d31', '--doc-muted': '#5f7264',
        '--doc-heading': '#1f3226', '--doc-link': '#2f6b3c', '--doc-strong': '#1f3226',
        '--doc-hr': '#aac9ad', '--doc-quote-bg': '#c2ddc4', '--doc-quote-bar': '#7fae86',
        '--doc-code-bg': '#c4dcc6', '--doc-code-fg': '#2e3d31', '--doc-code-border': '#aac9ad',
        '--doc-table-border': '#9dbfa2', '--doc-table-head-bg': '#c2ddc4', '--doc-table-alt-bg': '#c8e2ca',
      },
    },
    {
      id: 'anye', name: '暗夜黑', dark: true,
      vars: {
        '--doc-bg': '#1e2127', '--doc-fg': '#d5d7dd', '--doc-muted': '#8b8e99',
        '--doc-heading': '#eef0f4', '--doc-link': '#7aa2f7', '--doc-strong': '#f0f2f6',
        '--doc-hr': '#33363f', '--doc-quote-bg': '#262931', '--doc-quote-bar': '#4a4d58',
        '--doc-code-bg': '#2a2d35', '--doc-code-fg': '#e0e2e8', '--doc-code-border': '#33363f',
        '--doc-table-border': '#3b3e49', '--doc-table-head-bg': '#262931', '--doc-table-alt-bg': '#232630',
        '--doc-mark-bg': 'rgba(250,200,60,0.35)',
      },
    },
    {
      id: 'word', name: '仿 Word', dark: false,
      vars: {
        '--doc-bg': '#ffffff', '--doc-fg': '#000000', '--doc-muted': '#595959',
        '--doc-heading': '#000000', '--doc-link': '#0563c1', '--doc-strong': '#000000',
        '--doc-hr': '#bfbfbf', '--doc-quote-bg': '#f2f2f2', '--doc-quote-bar': '#7f7f7f',
        '--doc-code-bg': '#f7f7f7', '--doc-code-fg': '#000000', '--doc-code-border': '#d9d9d9',
        '--doc-table-border': '#000000', '--doc-table-head-bg': '#f2f2f2', '--doc-table-alt-bg': '#ffffff',
        '--doc-font-body': 'SimSun, "Songti SC", serif',
        '--doc-font-heading': 'SimHei, "Heiti SC", sans-serif',
        '--doc-indent': '2em', '--doc-align': 'justify',
        '--doc-h1-size': '1.6em', '--doc-h2-size': '1.35em', '--doc-h3-size': '1.15em',
        '--doc-h2-border': 'none',
      },
    },
    {
      id: 'lunwen', name: '论文衬线', dark: false,
      vars: {
        '--doc-bg': '#ffffff', '--doc-fg': '#1a1a1a', '--doc-muted': '#666666',
        '--doc-heading': '#000000', '--doc-link': '#1a4d8f', '--doc-strong': '#000000',
        '--doc-hr': '#cccccc', '--doc-quote-bg': '#fafafa', '--doc-quote-bar': '#999999',
        '--doc-code-bg': '#f5f5f5', '--doc-code-fg': '#1a1a1a', '--doc-code-border': '#dddddd',
        '--doc-table-border': '#999999', '--doc-table-head-bg': '#f0f0f0', '--doc-table-alt-bg': '#fafafa',
        '--doc-font-body': 'Georgia, "Times New Roman", SimSun, serif',
        '--doc-font-heading': 'Georgia, "Times New Roman", SimHei, serif',
        '--doc-align': 'justify', '--doc-line-height': '1.6',
        '--doc-h1-size': '1.55em', '--doc-h2-size': '1.3em', '--doc-h3-size': '1.12em',
      },
    },
    {
      id: 'shuju', name: '书卷衬线', dark: false,
      vars: {
        '--doc-bg': '#f7f2e8', '--doc-fg': '#3d3629', '--doc-muted': '#8c8271',
        '--doc-heading': '#5b3a1e', '--doc-link': '#8a5a2b', '--doc-strong': '#403020',
        '--doc-hr': '#ddd2bd', '--doc-quote-bg': '#f1e9d8', '--doc-quote-bar': '#c0a878',
        '--doc-code-bg': '#efe7d5', '--doc-code-fg': '#3d3629', '--doc-code-border': '#ddd2bd',
        '--doc-table-border': '#cfc2a6', '--doc-table-head-bg': '#f1e9d8', '--doc-table-alt-bg': '#f5efe2',
        '--doc-font-body': 'KaiTi, "Kaiti SC", Georgia, serif',
        '--doc-font-heading': 'KaiTi, "Kaiti SC", serif',
        '--doc-line-height': '1.9',
      },
    },
    {
      id: 'jianhui', name: '极简灰', dark: false,
      vars: {
        '--doc-bg': '#fafafa', '--doc-fg': '#374151', '--doc-muted': '#9ca3af',
        '--doc-heading': '#111827', '--doc-link': '#374151', '--doc-strong': '#111827',
        '--doc-hr': '#e5e7eb', '--doc-quote-bg': '#f3f4f6', '--doc-quote-bar': '#d1d5db',
        '--doc-code-bg': '#f3f4f6', '--doc-code-fg': '#374151', '--doc-code-border': '#e5e7eb',
        '--doc-table-border': '#e5e7eb', '--doc-table-head-bg': '#f9fafb', '--doc-table-alt-bg': '#fcfcfd',
        '--doc-font-body': '-apple-system, "Segoe UI", "Microsoft YaHei", sans-serif',
        '--doc-font-heading': '-apple-system, "Segoe UI", "Microsoft YaHei", sans-serif',
        '--doc-h2-border': 'none',
      },
    },
    {
      id: 'lanbo', name: '蓝图科技', dark: false,
      vars: {
        '--doc-bg': '#ffffff', '--doc-fg': '#1f2937', '--doc-muted': '#64748b',
        '--doc-heading': '#1e3a8a', '--doc-link': '#2563eb', '--doc-strong': '#111827',
        '--doc-hr': '#dbeafe', '--doc-quote-bg': '#eff6ff', '--doc-quote-bar': '#3b82f6',
        '--doc-code-bg': '#f0f6ff', '--doc-code-fg': '#1e3a8a', '--doc-code-border': '#dbeafe',
        '--doc-table-border': '#bfdbfe', '--doc-table-head-bg': '#dbeafe', '--doc-table-alt-bg': '#f4f9ff',
        '--doc-font-heading': '"Microsoft YaHei", sans-serif',
      },
    },
    {
      id: 'yingtao', name: '樱粉', dark: false,
      vars: {
        '--doc-bg': '#fff7f9', '--doc-fg': '#433038', '--doc-muted': '#a3848f',
        '--doc-heading': '#be3455', '--doc-link': '#d05577', '--doc-strong': '#8e2440',
        '--doc-hr': '#f5dbe3', '--doc-quote-bg': '#fdeef2', '--doc-quote-bar': '#ec9db4',
        '--doc-code-bg': '#fceef2', '--doc-code-fg': '#8e2440', '--doc-code-border': '#f5dbe3',
        '--doc-table-border': '#f0cdd7', '--doc-table-head-bg': '#fdeef2', '--doc-table-alt-bg': '#fef6f8',
      },
    },
  ];

  /* ── 文档样式默认值（唯一来源，写在 #preview-stage 上让 .markdown-body 继承） ── */
  const DOC_DEFAULTS = {
    '--doc-bg': '#ffffff', '--doc-fg': '#24292f', '--doc-muted': '#6b7280',
    '--doc-heading': '#111827', '--doc-link': '#2563eb', '--doc-strong': '#111827',
    '--doc-hr': '#d0d7de', '--doc-quote-bg': '#f6f8fa', '--doc-quote-bar': '#d0d7de',
    '--doc-code-bg': '#f3f4f6', '--doc-code-fg': '#24292f', '--doc-code-border': '#e5e7eb',
    '--doc-table-border': '#d0d7de', '--doc-table-head-bg': '#f6f8fa', '--doc-table-alt-bg': '#f9fafb',
    '--doc-mark-bg': 'rgba(255, 212, 0, 0.4)',
    '--doc-font-body': '"Microsoft YaHei", "Segoe UI", sans-serif',
    '--doc-font-heading': 'var(--doc-font-body)',
    '--doc-font-mono': 'Consolas, "Courier New", monospace',
    '--doc-font-size': '16px',
    '--doc-line-height': '1.8',
    '--doc-para-gap': '1em',
    '--doc-indent': '0em',
    '--doc-align': 'left',
    '--doc-max-width': '860px',
    '--doc-padding': '42px 56px',
    '--doc-h1-size': '1.9em',
    '--doc-h2-size': '1.5em',
    '--doc-h3-size': '1.24em',
    '--doc-h4-size': '1.1em',
    '--doc-heading-gap-top': '1.6em',
    '--doc-h2-border': '1px solid var(--doc-hr)',
    '--doc-radius': '8px',
    '--doc-shadow': 'none',
    '--doc-page-width': 'auto',
    '--doc-page-margin': '0px',
  };

  const PAPERS = {
    flow: { label: '流式（无纸张）' },
    A4: { label: 'A4 纵向', w: 210, h: 297 },
    A5: { label: 'A5 纵向', w: 148, h: 210 },
    B5: { label: 'B5 纵向', w: 176, h: 250 },
    letter: { label: 'Letter', w: 216, h: 279 },
    '16k': { label: '16K（184×260）', w: 184, h: 260 },
  };

  /* 上一次写到 #preview-stage 上的变量名（CSSStyleDeclaration 没有标准的枚举 API，
     自己记录，导出和清理都靠它） */
  let appliedVars = [];

  function getTheme(id) {
    if (MD.settings.customThemes[id]) return MD.settings.customThemes[id];
    return BUILTIN.find((t) => t.id === id) || BUILTIN[0];
  }
  function allThemes() {
    return [...BUILTIN, ...Object.values(MD.settings.customThemes)];
  }
  function isDark(id) { return !!getTheme(id).dark; }

  /* ── 应用 ── */
  function applyUI() {
    const mode = MD.settings.uiTheme === 'dark' ? 'dark' : 'light';
    document.documentElement.dataset.ui = mode;
    const lightCss = U.$('#hljs-light');
    const darkCss = U.$('#hljs-dark');
    const docDark = isDark(MD.settings.docTheme);
    if (lightCss) lightCss.disabled = docDark;
    if (darkCss) darkCss.disabled = !docDark;
    // mermaid 的配色在 initialize 时确定，界面切明暗后要重新设置，否则图表仍是旧配色
    if (MD.md && MD.md.refreshMermaidTheme) MD.md.refreshMermaidTheme();
  }

  /** 把主题的 extraCSS 作用域限定到 #preview-stage 内：
   *  普通规则选择器加前缀，@media/@supports 递归处理内部规则，
   *  @keyframes/@font-face 等原样保留；CSSOM 解析失败时退化为输出原文。 */
  function scopeExtraCss(css) {
    if (!css || !String(css).trim()) return '';
    let temp = null;
    try {
      temp = document.createElement('style');
      temp.textContent = css;
      document.head.appendChild(temp);
      const sheet = temp.sheet;
      if (!sheet || !sheet.cssRules) throw new Error('无法读取 cssRules');
      const prefix = (selectorText) => selectorText.split(',').map((s) => {
        const sel = s.trim();
        if (!sel || sel.indexOf('#preview-stage') === 0) return sel;
        return '#preview-stage ' + sel;
      }).join(', ');
      const out = [];
      const walk = (rules, indent) => {
        for (const r of rules) {
          if (r.type === CSSRule.STYLE_RULE) {
            out.push(indent + prefix(r.selectorText) + ' { ' + r.style.cssText + ' }');
          } else if (r.type === CSSRule.MEDIA_RULE || r.type === CSSRule.SUPPORTS_RULE) {
            const head = r.type === CSSRule.MEDIA_RULE
              ? '@media ' + r.conditionText
              : '@supports ' + r.conditionText;
            out.push(indent + head + ' {');
            walk(r.cssRules, indent + '  ');
            out.push(indent + '}');
          } else {
            // @keyframes / @font-face / @import 等保持原样，不加作用域前缀
            out.push(indent + r.cssText);
          }
        }
      };
      walk(sheet.cssRules, '');
      return out.join('\n');
    } catch (e) {
      return String(css);
    } finally {
      if (temp && temp.parentNode) temp.parentNode.removeChild(temp);
    }
  }

  /** 应用内预览：维护 <style id="theme-extra">，让主题的附加 CSS 真正生效 */
  function applyExtraCss() {
    let el = U.$('#theme-extra');
    if (!el) {
      el = document.createElement('style');
      el.id = 'theme-extra';
      document.head.appendChild(el);
    }
    const theme = getTheme(MD.settings.docTheme);
    el.textContent = scopeExtraCss(theme.extraCSS || '');
  }

  function applyDoc() {
    const theme = getTheme(MD.settings.docTheme);
    const stage = U.$('#preview-stage');
    const s = MD.settings;
    const vars = Object.assign({}, DOC_DEFAULTS, theme.vars);

    // 用户排版覆盖（空值 = 跟随主题）
    vars['--doc-font-size'] = (s.fontSizeDoc || 16) + 'px';
    if (s.lineHeightDoc !== '' && s.lineHeightDoc != null) {
      vars['--doc-line-height'] = String(s.lineHeightDoc);
    }
    vars['--doc-para-gap'] = (s.paraGapDoc != null ? s.paraGapDoc : 1) + 'em';
    if (s.indentDoc !== '' && s.indentDoc != null) vars['--doc-indent'] = s.indentDoc + 'em';
    if (s.alignDoc) vars['--doc-align'] = s.alignDoc;
    if (s.fontBody) vars['--doc-font-body'] = s.fontBody;
    if (s.fontHeading) vars['--doc-font-heading'] = s.fontHeading;
    if (s.fontMono) vars['--doc-font-mono'] = s.fontMono;
    if (s.maxWidthDoc) vars['--doc-max-width'] = s.maxWidthDoc + 'px';
    if (s.accentColor) {
      vars['--doc-link'] = s.accentColor;
      vars['--doc-heading'] = s.accentColor;
    }
    // 标题倍率：主题自带标题字号时按比例缩放，否则基于默认层级计算
    const hs = s.headingScale || 1;
    const base = { h1: 1.9, h2: 1.5, h3: 1.24, h4: 1.1 };
    const num = (k) => parseFloat(theme.vars[k]);
    vars['--doc-h1-size'] = ((Number.isFinite(num('--doc-h1-size')) ? num('--doc-h1-size') : base.h1) * hs) + 'em';
    vars['--doc-h2-size'] = ((Number.isFinite(num('--doc-h2-size')) ? num('--doc-h2-size') : base.h2) * hs) + 'em';
    vars['--doc-h3-size'] = ((Number.isFinite(num('--doc-h3-size')) ? num('--doc-h3-size') : base.h3) * hs) + 'em';
    vars['--doc-h4-size'] = ((Number.isFinite(num('--doc-h4-size')) ? num('--doc-h4-size') : base.h4) * hs) + 'em';

    // 清空再赋值，避免残留
    for (const name of appliedVars) stage.style.removeProperty(name);
    appliedVars = [];
    for (const [k, v] of Object.entries(vars)) {
      stage.style.setProperty(k, v);
      appliedVars.push(k);
    }

    // 纸张模式
    const paper = PAPERS[s.paper] ? s.paper : 'flow';
    stage.classList.toggle('paper', paper !== 'flow');
    if (paper !== 'flow') {
      const p = PAPERS[paper];
      const m = s.margins;
      stage.style.setProperty('--doc-page-width', p.w + 'mm');
      stage.style.setProperty('--doc-page-height', p.h + 'mm');
      stage.style.setProperty('--doc-max-width', p.w + 'mm');
      stage.style.setProperty('--doc-padding', `${m.top}mm ${m.right}mm ${m.bottom}mm ${m.left}mm`);
      if (!appliedVars.includes('--doc-page-width')) appliedVars.push('--doc-page-width', '--doc-page-height');
    } else {
      stage.style.removeProperty('--doc-page-width');
      stage.style.removeProperty('--doc-page-height');
      // 流式模式：四周留白（px）
      const fp = s.flowPadding != null ? s.flowPadding : 56;
      stage.style.setProperty('--doc-padding', `${Math.round(fp * 0.75)}px ${fp}px`);
      if (s.maxWidthDoc) stage.style.setProperty('--doc-max-width', s.maxWidthDoc + 'px');
    }

    // @page 打印规则
    let pageStyle = U.$('#page-style');
    if (!pageStyle) {
      pageStyle = document.createElement('style');
      pageStyle.id = 'page-style';
      document.head.appendChild(pageStyle);
    }
    if (paper !== 'flow') {
      const p = PAPERS[paper];
      const m = s.margins;
      pageStyle.textContent = `@page { size: ${p.w}mm ${p.h}mm; margin: ${m.top}mm ${m.right}mm ${m.bottom}mm ${m.left}mm; }`;
    } else {
      const fp = s.flowPadding != null ? s.flowPadding : 56;
      pageStyle.textContent = `@page { margin: ${Math.round(fp * 0.75)}px ${fp}px; }`;
    }
    applyExtraCss();
    applyUI();
    MD.bus.emit('theme:applied');
  }

  /* ── 导出用：取当前主题的变量集（含默认值，保证导出的 HTML 独立可用） ── */
  function resolvedVars() {
    const theme = getTheme(MD.settings.docTheme);
    const stage = U.$('#preview-stage');
    const vars = Object.assign({}, DOC_DEFAULTS, theme.vars);
    // 排版面板 / 纸张模式写在 stage 上的实时值优先（含用户覆盖）
    for (const name of appliedVars) {
      const v = stage.style.getPropertyValue(name);
      if (v) vars[name] = v;
    }
    const pageStyle = U.$('#page-style');
    return {
      vars,
      extraCSS: theme.extraCSS || '',
      name: theme.name,
      dark: !!theme.dark,
      pageCss: pageStyle ? pageStyle.textContent : '',
      paper: MD.settings.paper || 'flow',
    };
  }

  /* ── 主题菜单 ── */
  function renderThemeMenu() {
    const pop = U.$('#theme-menu-pop');
    const s = MD.settings;
    const swatch = (t) => `<span class="theme-dot" style="background:${t.vars['--doc-bg'] || '#fff'};border-color:${t.dark ? '#555' : '#ccc'}"></span>`;
    let html = '<div class="menu-label">界面</div>';
    html += `<button data-ui-set="light" class="${s.uiTheme === 'light' ? 'checked' : ''}">浅色界面</button>`;
    html += `<button data-ui-set="dark" class="${s.uiTheme === 'dark' ? 'checked' : ''}">深色界面</button>`;
    html += '<hr><div class="menu-label">文档主题</div>';
    for (const t of BUILTIN) {
      html += `<button data-theme-set="${t.id}" class="${s.docTheme === t.id ? 'checked' : ''}"><span class="theme-swatch">${swatch(t)}${t.name}</span></button>`;
    }
    const customs = Object.values(s.customThemes || {});
    if (customs.length) {
      html += '<hr><div class="menu-label">我的主题</div>';
      for (const t of customs) {
        html += `<button data-theme-set="${t.id}" class="${s.docTheme === t.id ? 'checked' : ''}"><span class="theme-swatch">${swatch(t)}${U.esc(t.name)}</span></button>`;
      }
    }
    html += '<hr><button data-act="importTheme">导入主题…</button>';
    html += '<button data-act="openThemeEditor">主题编辑器…</button>';
    pop.innerHTML = html;
  }

  /* ── 主题导入 ── */
  /** 校验导入对象并返回规范化主题；纯函数，不写入任何设置。
   *  与 exportTheme() 导出的主题对象结构兼容：{ id, name, dark, vars, base?, extraCSS? }。
   *  id 忽略并重新生成，避免与内置主题 / 现有自定义主题冲突。 */
  function parseImportedTheme(raw) {
    let obj;
    try { obj = JSON.parse(raw); }
    catch (e) { return { ok: false, error: '不是有效的 JSON 文件' }; }
    if (!obj || typeof obj !== 'object' || Array.isArray(obj)) {
      return { ok: false, error: '主题格式不正确（应为对象）' };
    }
    const name = typeof obj.name === 'string' ? obj.name.trim() : '';
    if (!name) return { ok: false, error: '缺少主题名称（name）' };
    if (!obj.vars || typeof obj.vars !== 'object' || Array.isArray(obj.vars)) {
      return { ok: false, error: '缺少主题变量（vars 对象）' };
    }
    // 只接受 --doc-* 变量且值必须是字符串，过滤非字符串值与明显非法的键
    const vars = {};
    for (const [k, v] of Object.entries(obj.vars)) {
      if (!/^--doc-[a-z0-9-]+$/i.test(k) || typeof v !== 'string') continue;
      vars[k] = v;
    }
    if (!Object.keys(vars).length) {
      return { ok: false, error: 'vars 中没有可用的 --doc-* 变量' };
    }
    const theme = {
      name,
      vars,
      extraCSS: typeof obj.extraCSS === 'string' ? obj.extraCSS : '',
      dark: !!obj.dark,
    };
    if (typeof obj.base === 'string' && obj.base.trim()) theme.base = obj.base.trim();
    return { ok: true, theme };
  }

  /** 生成不与内置主题 / 现有自定义主题冲突的 id */
  function uniqueThemeId() {
    const builtin = new Set(BUILTIN.map((t) => t.id));
    let id;
    do { id = 'custom-' + U.uid(); }
    while (builtin.has(id) || (MD.settings.customThemes && MD.settings.customThemes[id]));
    return id;
  }

  /** 读取文件文本：File.text 不可用时回退 FileReader */
  function readThemeFile(file) {
    return new Promise((resolve, reject) => {
      if (typeof file.text === 'function') { file.text().then(resolve, reject); return; }
      const fr = new FileReader();
      fr.onload = () => resolve(fr.result);
      fr.onerror = () => reject(fr.error || new Error('读取文件失败'));
      fr.readAsText(file, 'utf-8');
    });
  }

  /**
   * 导入主题：用隐藏的 <input type="file"> 直接拿到文件内容。
   * 不复用 MD.fsx.pickFile —— 它返回的是路径，浏览器模式还固定只接受 markdown 类型，
   * 且 readFile 会写入 fileMeta/编码信息，对纯 UTF-8 的 .json 主题并不合适。
   * <input> 在 Tauri(WebView2) 与浏览器都可用，且不涉及内联脚本，严格 CSP 兼容。
   */
  function importTheme() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json,application/json';
    input.style.display = 'none';
    document.body.appendChild(input);
    const cleanup = () => { if (input.parentNode) input.parentNode.removeChild(input); };
    input.addEventListener('change', async () => {
      const file = input.files && input.files[0];
      if (!file) { cleanup(); return; }   // 用户取消选择
      try {
        const text = await readThemeFile(file);
        const parsed = parseImportedTheme(text);
        if (!parsed.ok) { MD.toast('导入失败：' + parsed.error); return; }
        // 全部校验通过后才写设置：JSON 损坏 / 结构不符 / 读写异常都不会污染现有设置
        if (!MD.settings.customThemes || typeof MD.settings.customThemes !== 'object') {
          MD.settings.customThemes = {};
        }
        const id = uniqueThemeId();
        const theme = parsed.theme;
        theme.id = id;
        MD.settings.customThemes[id] = theme;
        MD.settings.docTheme = id;
        MD.saveSettings();
        applyDoc();                    // 立即应用（内部会 emit theme:applied）
        renderThemeMenu();             // 刷新主题菜单
        MD.bus.emit('render:needed');  // 与切换主题一致：刷新代码高亮明暗配色
        MD.toast('主题已导入：' + theme.name);
      } catch (e) {
        MD.toast('导入失败：' + (e && e.message ? e.message : e));
      } finally {
        cleanup();                     // 无论成功失败都移除临时 input，不残留节点
      }
    });
    input.click();
  }

  /* ══════════ 排版面板 ══════════ */
  // 排版改动都是纯 CSS 变量，正常情况下不需要重新渲染 Markdown；
  // 只有开着分页预览时才必须重排，否则 paged.js 的页框尺寸会跟正文对不上。
  let _pagedTimer = null;
  function refreshPaged() {
    if (!(MD.settings.pagedPreview && MD.settings.paper !== 'flow')) return;
    clearTimeout(_pagedTimer);
    _pagedTimer = setTimeout(() => MD.bus.emit('render:needed'), 320);
  }
  function applyTypography() {
    applyDoc();
    MD.saveSettings();
    refreshPaged();
  }

  function renderLayoutPanel() {
    const s = MD.settings;
    const body = U.$('#layout-body');
    const themeVars = getTheme(s.docTheme).vars;
    const fontOpts = (list, cur) => list.map((f) =>
      `<option value="${U.esc(f.v)}" ${f.v === cur ? 'selected' : ''}>${f.label}</option>`).join('');
    const paperOpts = Object.entries(PAPERS).map(([k, p]) =>
      `<option value="${k}" ${s.paper === k ? 'selected' : ''}>${p.label}</option>`).join('');
    // 行距支持「跟随主题」：空串表示跟随，滑块显示主题当前值
    const lhFollow = s.lineHeightDoc === '' || s.lineHeightDoc == null;
    const lhTheme = parseFloat(themeVars['--doc-line-height']) || 1.8;
    const lhVal = lhFollow ? lhTheme : s.lineHeightDoc;

    body.innerHTML = `
      <div class="ctl-title">纸张</div>
      <div class="ctl-row"><label>纸张尺寸</label><select id="ly-paper">${paperOpts}</select></div>
      <div class="ctl-row" id="ly-margins-row"><label>页边距 mm</label>
        <input type="number" id="ly-mt" min="0" max="60" value="${s.margins.top}" title="上">
        <input type="number" id="ly-ml" min="0" max="60" value="${s.margins.left}" title="左">
        <input type="number" id="ly-mr" min="0" max="60" value="${s.margins.right}" title="右">
        <input type="number" id="ly-mb" min="0" max="60" value="${s.margins.bottom}" title="下">
      </div>
      <div class="ctl-row" id="ly-flowpad-row"><label>四周留白</label><input type="range" id="ly-flowpad" min="0" max="120" step="4" value="${s.flowPadding != null ? s.flowPadding : 56}"><span class="val" id="ly-flowpad-val">${s.flowPadding != null ? s.flowPadding : 56}px</span></div>
      <div class="ctl-row"><label>分页预览</label><label class="chk"><input type="checkbox" id="ly-paged" ${s.pagedPreview ? 'checked' : ''}>按纸张分页显示（paged.js）</label></div>
      <hr class="ctl-sep">
      <div class="ctl-title">段落与文本</div>
      <div class="ctl-row"><label>正文字号</label><input type="range" id="ly-fs" min="12" max="26" step="0.5" value="${s.fontSizeDoc}"><span class="val" id="ly-fs-val">${s.fontSizeDoc}px</span></div>
      <div class="ctl-row"><label>行距</label>
        <input type="range" id="ly-lh" min="1.2" max="2.6" step="0.05" value="${lhVal}" ${lhFollow ? 'disabled' : ''}><span class="val" id="ly-lh-val">${lhFollow ? '主题' : lhVal}</span>
      </div>
      <div class="ctl-row" style="margin-top:-6px"><label></label>
        <label class="chk"><input type="checkbox" id="ly-lh-follow" ${lhFollow ? 'checked' : ''}>跟随主题（当前 ${lhTheme}）</label>
      </div>
      <div class="ctl-row"><label>段落间距</label><input type="range" id="ly-pg" min="0.2" max="2.4" step="0.1" value="${s.paraGapDoc}"><span class="val" id="ly-pg-val">${s.paraGapDoc}em</span></div>
      <div class="ctl-row"><label>首行缩进</label><select id="ly-indent">
        <option value="" ${s.indentDoc === '' || s.indentDoc == null ? 'selected' : ''}>跟随主题</option>
        <option value="0" ${s.indentDoc === 0 ? 'selected' : ''}>无</option>
        <option value="1" ${s.indentDoc === 1 ? 'selected' : ''}>1em</option>
        <option value="2" ${s.indentDoc === 2 ? 'selected' : ''}>2em（中文习惯）</option>
      </select></div>
      <div class="ctl-row"><label>对齐方式</label><select id="ly-align">
        <option value="" ${!s.alignDoc ? 'selected' : ''}>跟随主题</option>
        <option value="left" ${s.alignDoc === 'left' ? 'selected' : ''}>左对齐</option>
        <option value="justify" ${s.alignDoc === 'justify' ? 'selected' : ''}>两端对齐</option>
      </select></div>
      <div class="ctl-row"><label>正文字体</label><select id="ly-fbody">${fontOpts(FONTS_BODY, s.fontBody)}</select></div>
      <div class="ctl-row"><label>标题字体</label><select id="ly-fhead">${fontOpts(FONTS_HEADING, s.fontHeading)}</select></div>
      <div class="ctl-row"><label>代码字体</label><select id="ly-fmono">${fontOpts(FONTS_MONO, s.fontMono)}</select></div>
      <hr class="ctl-sep">
      <div class="ctl-title">其他</div>
      <div class="ctl-row"><label>内容宽度</label><input type="range" id="ly-mw" min="560" max="1400" step="20" value="${s.maxWidthDoc}"><span class="val" id="ly-mw-val">${s.maxWidthDoc}px</span></div>
      <div class="ctl-row"><label>标题倍率</label><input type="range" id="ly-hs" min="0.8" max="1.4" step="0.05" value="${s.headingScale}"><span class="val" id="ly-hs-val">×${s.headingScale}</span></div>
      <div class="ctl-row"><label>强调色</label><input type="color" id="ly-accent" value="${s.accentColor || '#4f46e5'}"><label class="chk"><input type="checkbox" id="ly-accent-off" ${!s.accentColor ? 'checked' : ''}>跟随主题</label></div>
      <div class="btn-row">
        <button class="btn" id="ly-reset">恢复默认</button>
      </div>
    `;

    const bindRange = (id, valId, key, fmt) => {
      const el = U.$('#' + id, body);
      const val = U.$('#' + valId, body);
      el.addEventListener('input', () => {
        MD.settings[key] = parseFloat(el.value);
        val.textContent = fmt(el.value);
        applyTypography();
      });
    };
    const syncPaperRows = (v) => {
      U.$('#ly-margins-row', body).hidden = v === 'flow';
      U.$('#ly-flowpad-row', body).hidden = v !== 'flow';
      // 纸张模式下内容宽度被纸张宽度接管，滑块禁用避免"拖了没反应"
      const mw = U.$('#ly-mw', body);
      if (mw) {
        mw.disabled = v !== 'flow';
        mw.title = v === 'flow' ? '' : '纸张模式下内容宽度由纸张尺寸决定';
      }
    };
    syncPaperRows(s.paper);
    U.$('#ly-paper', body).addEventListener('change', (e) => {
      MD.settings.paper = e.target.value;
      syncPaperRows(e.target.value);
      applyDoc(); MD.saveSettings(); MD.bus.emit('render:needed');
    });
    ['mt:top', 'ml:left', 'mr:right', 'mb:bottom'].forEach((pair) => {
      const [id, key] = pair.split(':');
      U.$('#ly-m' + id[1], body).addEventListener('change', (e) => {
        MD.settings.margins[key] = U.clamp(parseInt(e.target.value) || 0, 0, 60);
        applyDoc(); MD.saveSettings(); MD.bus.emit('render:needed');
      });
    });
    U.$('#ly-paged', body).addEventListener('change', (e) => {
      MD.settings.pagedPreview = e.target.checked;
      MD.saveSettings(); MD.bus.emit('render:needed');
    });
    bindRange('ly-flowpad', 'ly-flowpad-val', 'flowPadding', (v) => v + 'px');
    bindRange('ly-fs', 'ly-fs-val', 'fontSizeDoc', (v) => v + 'px');
    bindRange('ly-pg', 'ly-pg-val', 'paraGapDoc', (v) => v + 'em');
    // 行距：勾「跟随主题」= 存空串；拖滑块 = 存数值并取消跟随
    const lhEl = U.$('#ly-lh', body);
    const lhValEl = U.$('#ly-lh-val', body);
    const lhFollowEl = U.$('#ly-lh-follow', body);
    lhFollowEl.addEventListener('change', () => {
      if (lhFollowEl.checked) {
        MD.settings.lineHeightDoc = '';
        lhEl.disabled = true;
        const v = parseFloat(getTheme(MD.settings.docTheme).vars['--doc-line-height']) || 1.8;
        lhEl.value = v;
        lhValEl.textContent = '主题';
      } else {
        lhEl.disabled = false;
        MD.settings.lineHeightDoc = parseFloat(lhEl.value);
        lhValEl.textContent = lhEl.value;
      }
      applyTypography();
    });
    lhEl.addEventListener('input', () => {
      if (lhFollowEl.checked) lhFollowEl.checked = false;
      lhEl.disabled = false;
      MD.settings.lineHeightDoc = parseFloat(lhEl.value);
      lhValEl.textContent = lhEl.value;
      applyTypography();
    });
    U.$('#ly-indent', body).addEventListener('change', (e) => {
      MD.settings.indentDoc = e.target.value === '' ? '' : parseFloat(e.target.value);
      applyTypography();
    });
    U.$('#ly-align', body).addEventListener('change', (e) => {
      MD.settings.alignDoc = e.target.value;
      applyTypography();
    });
    U.$('#ly-fbody', body).addEventListener('change', (e) => { MD.settings.fontBody = e.target.value; applyTypography(); });
    U.$('#ly-fhead', body).addEventListener('change', (e) => { MD.settings.fontHeading = e.target.value; applyTypography(); });
    U.$('#ly-fmono', body).addEventListener('change', (e) => { MD.settings.fontMono = e.target.value; applyTypography(); });
    bindRange('ly-mw', 'ly-mw-val', 'maxWidthDoc', (v) => v + 'px');
    bindRange('ly-hs', 'ly-hs-val', 'headingScale', (v) => '×' + v);
    const accent = U.$('#ly-accent', body);
    const accentOff = U.$('#ly-accent-off', body);
    accent.addEventListener('input', () => {
      accentOff.checked = false;
      MD.settings.accentColor = accent.value;
      applyTypography();
    });
    accentOff.addEventListener('change', () => {
      if (accentOff.checked) { MD.settings.accentColor = ''; applyTypography(); }
    });
    U.$('#ly-reset', body).addEventListener('click', () => {
      const d = MD.defaults;
      Object.assign(MD.settings, {
        paper: d.paper, fontSizeDoc: d.fontSizeDoc, lineHeightDoc: d.lineHeightDoc,
        paraGapDoc: d.paraGapDoc, indentDoc: d.indentDoc, alignDoc: d.alignDoc,
        fontBody: '', fontHeading: '', fontMono: '', maxWidthDoc: d.maxWidthDoc,
        headingScale: d.headingScale, accentColor: '', pagedPreview: false,
        flowPadding: d.flowPadding,
        margins: Object.assign({}, d.margins),
      });
      renderLayoutPanel(); applyDoc(); MD.saveSettings(); MD.bus.emit('render:needed');
      MD.toast('排版设置已恢复默认');
    });
  }

  /* ══════════ 主题编辑器 ══════════ */
  let editingThemeId = null;

  const THEME_FIELDS = [
    ['--doc-bg', '背景色', 'color'],
    ['--doc-fg', '正文颜色', 'color'],
    ['--doc-heading', '标题颜色', 'color'],
    ['--doc-link', '链接颜色', 'color'],
    ['--doc-muted', '次要文字', 'color'],
    ['--doc-strong', '加粗颜色', 'color'],
    ['--doc-hr', '分隔线', 'color'],
    ['--doc-quote-bg', '引用背景', 'color'],
    ['--doc-quote-bar', '引用竖条', 'color'],
    ['--doc-code-bg', '代码背景', 'color'],
    ['--doc-code-fg', '代码颜色', 'color'],
    ['--doc-code-border', '代码边框', 'color'],
    ['--doc-table-border', '表格边框', 'color'],
    ['--doc-table-head-bg', '表头背景', 'color'],
    ['--doc-table-alt-bg', '隔行背景', 'color'],
    ['--doc-mark-bg', '高亮标记', 'color'],
  ];

  function openThemeEditor(id) {
    editingThemeId = id || null;
    U.$('#theme-modal').hidden = false;
    renderThemeEditor();
  }
  function closeThemeEditor() { U.$('#theme-modal').hidden = true; }

  function renderThemeEditor() {
    const s = MD.settings;
    const list = U.$('#theme-ed-list') || document.createElement('div');
    const body = U.$('#theme-body');

    const themes = allThemes();
    const cur = editingThemeId ? getTheme(editingThemeId) : null;
    const isCustom = editingThemeId && !!s.customThemes[editingThemeId];

    const itemsHtml = themes.map((t) => `
      <button class="btn ${editingThemeId === t.id ? 'active' : ''}" data-pick="${t.id}">${U.esc(t.name)}${s.customThemes[t.id] ? '（自定义）' : ''}</button>
    `).join('');

    const formHtml = cur ? `
      <div class="ctl-row"><label>主题名称</label><input type="text" id="te-name" value="${U.esc(cur.name)}"></div>
      <div class="ctl-row"><label>基于主题</label><span style="color:var(--ui-muted)">${isCustom ? (s.customThemes[editingThemeId].base || '—') : '内置主题'}</span></div>
      <div class="ctl-title">颜色</div>
      ${THEME_FIELDS.map(([k, label]) => `
        <div class="ctl-row"><label>${label}</label><input type="color" data-te="${k}" data-orig="${U.esc(cur.vars[k] || '')}" value="${colorInputVal(cur.vars[k] || '#ffffff')}"></div>
      `).join('')}
      <hr class="ctl-sep">
      <div class="ctl-title">字体</div>
      <div class="ctl-row"><label>正文字体</label><select id="te-font-body">${FONTS_BODY.slice(1).map((f) => `<option value="${U.esc(f.v)}" ${cur.vars['--doc-font-body'] === f.v ? 'selected' : ''}>${f.label}</option>`).join('')}</select></div>
      <div class="ctl-row"><label>标题字体</label><select id="te-font-head">${FONTS_HEADING.map((f) => `<option value="${U.esc(f.v)}" ${cur.vars['--doc-font-heading'] === f.v ? 'selected' : ''}>${f.label}</option>`).join('')}</select></div>
      <div class="ctl-row"><label>代码字体</label><select id="te-font-mono">${FONTS_MONO.slice(1).map((f) => `<option value="${U.esc(f.v)}" ${cur.vars['--doc-font-mono'] === f.v ? 'selected' : ''}>${f.label}</option>`).join('')}</select></div>
      <div class="ctl-row"><label>深色主题</label><label class="chk"><input type="checkbox" id="te-dark" ${cur.dark ? 'checked' : ''}>代码高亮使用深色配色</label></div>
      <hr class="ctl-sep">
      <div class="ctl-title">进阶（附加 CSS）</div>
      <textarea id="te-extra" style="width:100%;min-height:110px;font-family:Consolas,monospace;font-size:12px;background:var(--ui-bg-2);color:var(--ui-fg);border:1px solid var(--ui-border);border-radius:6px;padding:8px" placeholder="/* 例如：h1 { letter-spacing: 2px; } */">${U.esc(cur.extraCSS || '')}</textarea>
      <div class="btn-row">
        ${isCustom ? '<button class="btn danger" id="te-delete">删除主题</button>' : ''}
        <button class="btn" id="te-save-new">另存为新主题</button>
        <button class="btn primary" id="te-save">${isCustom ? '保存修改' : '以此为基础创建'}</button>
      </div>
    ` : `
      <div style="color:var(--ui-muted);padding:30px 10px;text-align:center;line-height:2">
        左侧选择一个主题进行修改，<br>修改会以它为基础生成你的自定义主题。
      </div>
    `;

    body.innerHTML = `
      <div class="theme-ed-grid">
        <div class="theme-ed-list" id="theme-ed-list">${itemsHtml}
          <button class="btn" id="te-new" style="margin-top:8px">＋ 新建空白主题</button>
        </div>
        <div class="theme-ed-form">${formHtml}</div>
      </div>
    `;

    U.$$('[data-pick]', body).forEach((btn) => {
      btn.addEventListener('click', () => { editingThemeId = btn.dataset.pick; renderThemeEditor(); });
    });
    const pickColor = (input) => {
      input.addEventListener('input', () => {
        const theme = getTheme(editingThemeId);
        previewDraft(input.dataset.te, input.value, theme);
      });
    };
    U.$$('[data-te]', body).forEach(pickColor);

    const collectDraft = () => {
      const base = JSON.parse(JSON.stringify(getTheme(editingThemeId)));
      base.name = U.$('#te-name', body).value.trim() || base.name;
      U.$$('[data-te]', body).forEach((inp) => {
        const orig = inp.dataset.orig || '';
        // 未被用户改动时保留原始值（含 rgba 透明度），避免 type="color" 只能显示
        // 6 位实色、保存时把半透明高亮压成实色
        if (orig && inp.value.toLowerCase() === colorInputVal(orig).toLowerCase()) return;
        base.vars[inp.dataset.te] = inp.value;
      });
      base.vars['--doc-font-body'] = U.$('#te-font-body', body).value;
      base.vars['--doc-font-heading'] = U.$('#te-font-head', body).value;
      base.vars['--doc-font-mono'] = U.$('#te-font-mono', body).value;
      base.dark = U.$('#te-dark', body).checked;
      base.extraCSS = U.$('#te-extra', body).value;
      return base;
    };
    const saveAsCustom = (overwriteId) => {
      const draft = collectDraft();
      const id = overwriteId || 'custom-' + U.uid();
      draft.id = id;
      draft.base = getTheme(editingThemeId).name;
      MD.settings.customThemes[id] = draft;
      MD.settings.docTheme = id;
      MD.saveSettings();
      applyDoc();
      renderThemeMenu();
      editingThemeId = id;
      renderThemeEditor();
      MD.toast('主题已保存：' + draft.name);
    };
    const saveBtn = U.$('#te-save', body);
    if (saveBtn) saveBtn.addEventListener('click', () => saveAsCustom(isCustom ? editingThemeId : null));
    const saveNewBtn = U.$('#te-save-new', body);
    if (saveNewBtn) saveNewBtn.addEventListener('click', () => saveAsCustom(null));
    const delBtn = U.$('#te-delete', body);
    if (delBtn) delBtn.addEventListener('click', () => {
      const t = MD.settings.customThemes[editingThemeId];
      if (!t) return;
      delete MD.settings.customThemes[editingThemeId];
      if (MD.settings.docTheme === editingThemeId) MD.settings.docTheme = 'qingjie';
      editingThemeId = null;
      MD.saveSettings(); applyDoc(); renderThemeMenu(); renderThemeEditor();
      MD.toast('已删除主题：' + t.name);
    });
    const newBtn = U.$('#te-new', body);
    newBtn.addEventListener('click', () => {
      const id = 'custom-' + U.uid();
      const base = JSON.parse(JSON.stringify(BUILTIN[0]));
      base.id = id; base.name = '我的主题 ' + (Object.keys(MD.settings.customThemes).length + 1);
      MD.settings.customThemes[id] = base;
      MD.saveSettings();
      editingThemeId = id;
      renderThemeEditor();
    });
  }

  // 编辑中实时预览（只改 stage 上的内联变量，绝不改动主题对象本身，
  // 否则未保存的拖动会永久污染内置主题）
  const _draftProps = {};
  function previewDraft(prop, value, theme) {
    const stage = U.$('#preview-stage');
    if (!_draftProps.init) {
      _draftProps.orig = {}; _draftProps.init = true;
    }
    if (_draftProps.orig[prop] === undefined) {
      _draftProps.orig[prop] = stage.style.getPropertyValue(prop) || theme.vars[prop] || '';
    }
    stage.style.setProperty(prop, value);
  }
  function clearDraftPreview() {
    if (_draftProps.init && _draftProps.orig) {
      const stage = U.$('#preview-stage');
      for (const [k, v] of Object.entries(_draftProps.orig)) {
        if (v) stage.style.setProperty(k, v);
        else stage.style.removeProperty(k);
      }
      _draftProps.orig = {}; _draftProps.init = false;
    }
    // 从主题对象重新算一遍，顺手清掉草稿改动
    applyDoc();
  }

  function toHex(c) {
    if (!c) return '#ffffff';
    const s = String(c).trim();
    if (s.startsWith('#')) {
      // 3 位 #abc → #aabbcc（type="color" 只认 6 位）
      if (/^#[0-9a-fA-F]{3}$/.test(s)) return '#' + s[1] + s[1] + s[2] + s[2] + s[3] + s[3];
      // 4 位 #rgba → #rrggbbaa，保留透明度
      if (/^#[0-9a-fA-F]{4}$/.test(s)) return '#' + s[1]+s[1] + s[2]+s[2] + s[3]+s[3] + s[4]+s[4];
      // 6 位 / 8 位原样返回（8 位保留 alpha）
      if (/^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(s)) return s;
      return '#ffffff';
    }
    const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(s);
    if (m) {
      const h = (n) => Math.max(0, Math.min(255, Math.round(+n || 0))).toString(16).padStart(2, '0');
      let out = '#' + h(m[1]) + h(m[2]) + h(m[3]);
      // 带 alpha 且非完全不透明：补 8 位 hex，避免保存后高亮色从半透明变实色
      if (m[4] != null && parseFloat(m[4]) < 1) out += h(Math.round(parseFloat(m[4]) * 255));
      return out;
    }
    return '#ffffff';
  }
  /** type="color" 只能显示 6 位实色，含透明度的 8 位 hex 先截成 6 位 */
  function colorInputVal(c) {
    const h = toHex(c || '#ffffff');
    return h.length === 9 ? h.slice(0, 7) : h;
  }

  /* ══════════ 设置面板 ══════════ */
  function renderSettings() {
    const s = MD.settings;
    const body = U.$('#settings-body');
    body.innerHTML = `
      <div class="ctl-title">界面</div>
      <div class="ctl-row"><label>界面明暗</label><select id="se-ui">
        <option value="light" ${s.uiTheme === 'light' ? 'selected' : ''}>浅色</option>
        <option value="dark" ${s.uiTheme === 'dark' ? 'selected' : ''}>深色</option>
      </select></div>
      <div class="ctl-row"><label>编辑器字号</label><input type="range" id="se-cmfs" min="10" max="28" step="0.5" value="${s.fontSize}"><span class="val" id="se-cmfs-val">${s.fontSize}px</span></div>
      <div class="ctl-row"><label>画面缩放</label><input type="range" id="se-zoom" min="50" max="250" step="5" value="${s.zoom || 100}"><span class="val" id="se-zoom-val">${s.zoom || 100}%</span></div>
      <div class="ctl-hint">按住 Ctrl 滚动鼠标滚轮也能缩放，Ctrl+0 一键回到 100%</div>
      <div class="ctl-row"><label>侧栏</label><label class="chk"><input type="checkbox" id="se-sidebar" ${s.sidebarOn ? 'checked' : ''}>显示侧栏</label></div>
      <hr class="ctl-sep">
      <div class="ctl-title">编辑</div>
      <div class="ctl-row"><label>自动保存</label><label class="chk"><input type="checkbox" id="se-autosave" ${s.autoSave ? 'checked' : ''}>修改 1.5 秒后自动写回文件</label></div>
      <div class="ctl-row"><label>打字机滚动</label><label class="chk"><input type="checkbox" id="se-tw" ${s.typewriter ? 'checked' : ''}>光标始终保持在屏幕中部</label></div>
      <div class="ctl-row"><label>粘贴图片</label><label class="chk"><input type="checkbox" id="se-img" ${s.pasteImageToLocal ? 'checked' : ''}>粘贴/拖入图片保存到文档旁 assets 文件夹</label></div>
      <div class="ctl-row"><label>启动恢复</label><label class="chk"><input type="checkbox" id="se-restore-tabs" ${s.restoreTabs ? 'checked' : ''}>启动时重新打开上次的标签页</label></div>
      <hr class="ctl-sep">
      <div class="ctl-title">文件关联</div>
      <div class="ctl-row"><label>.md 打开方式</label>
        <button class="btn" id="se-assoc">用 MDeditor 打开 .md</button>
        <span class="val" id="se-assoc-state" style="width:auto;flex:1;text-align:left">检查中…</span>
      </div>
      <div style="color:var(--ui-muted);font-size:12px;line-height:1.8;margin:-4px 0 6px 102px">
        点按钮把本程序登记为 .md / .markdown 的候选程序，然后会自动打开 Windows「默认应用」设置，
        在里面把 .md 的默认应用选成 MDeditor 即可（Win10 之后系统不允许软件直接改写默认程序）。
      </div>
      <hr class="ctl-sep">
      <div class="ctl-title">关于</div>
      <div style="color:var(--ui-muted);line-height:2;font-size:12.5px">
        MDeditor <span id="se-version">v1.6</span> —— 本地 Markdown 阅读与排版编辑器<br>
        运行环境：${MD.fsx.isApp ? 'MDeditor 桌面应用（Tauri）' : '浏览器模式（部分功能受限）'}<br>
        所有文件均保存在本地，不上传任何数据。<br>
        渲染内核：marked + highlight.js + KaTeX + Mermaid + paged.js
      </div>
      <div class="author-card">
        <div class="author-row"><span class="author-label">作者</span><b>行一</b></div>
        <div class="author-tip">想发现更多有意思的小工具，可以关注公众号「每天AI进步」</div>
      </div>
      <div class="disclaimer">
        <b>免责声明</b><br>
        本软件按 <b>MIT 许可证</b>发布（见 LICENSE），允许个人与商业使用、复制、修改和分发，
        使用时须保留版权与许可声明。<br>
        软件按「现状」提供，不对适用性、稳定性、兼容性及数据安全作任何明示或默示的担保；
        请在使用前自行备份重要文件，并对编辑与导出的结果自行核对。<br>
        使用者因使用或无法使用本软件而产生的任何直接或间接损失（含数据丢失、业务中断、利润损失），
        均由使用者自行承担，开发者不承担任何责任。
      </div>
      <div class="btn-row"><button class="btn" id="se-help">查看快捷键</button></div>
    `;
    // 版本号以 Cargo.toml / tauri.conf.json 为唯一来源，不再在界面里写死
    const verEl = U.$('#se-version', body);
    if (verEl && MD.fsx.appVersion) {
      MD.fsx.appVersion().then((v) => { if (v) verEl.textContent = 'v' + v; }).catch(() => {});
    }
    U.$('#se-ui', body).addEventListener('change', (e) => { MD.settings.uiTheme = e.target.value; applyDoc(); MD.saveSettings(); });
    U.$('#se-cmfs', body).addEventListener('input', (e) => {
      const px = parseFloat(e.target.value);
      MD.settings.fontSize = px;
      U.$('#se-cmfs-val', body).textContent = px + 'px';
      MD.editor.applyFontSize(px);
      MD.saveSettings();
    });
    U.$('#se-zoom', body).addEventListener('input', (e) => {
      const v = parseInt(e.target.value, 10);
      U.$('#se-zoom-val', body).textContent = v + '%';
      MD.zoom.set(v, { silent: true });
    });
    U.$('#se-sidebar', body).addEventListener('change', (e) => { MD.app.toggleSidebar(e.target.checked); });
    U.$('#se-autosave', body).addEventListener('change', (e) => { MD.settings.autoSave = e.target.checked; MD.saveSettings(); });
    U.$('#se-tw', body).addEventListener('change', (e) => { MD.app.setTypewriter(e.target.checked); });
    U.$('#se-img', body).addEventListener('change', (e) => { MD.settings.pasteImageToLocal = e.target.checked; MD.saveSettings(); });
    U.$('#se-restore-tabs', body).addEventListener('change', (e) => { MD.settings.restoreTabs = e.target.checked; MD.saveSettings(); });
    U.$('#se-help', body).addEventListener('click', () => { U.$('#settings-modal').hidden = true; MD.app.showHelp(); });

    // 文件关联
    const assocState = U.$('#se-assoc-state', body);
    const fmtAssoc = (ok) => {
      assocState.textContent = ok ? '已登记为候选程序' : '尚未关联';
      assocState.style.color = ok ? 'var(--ui-accent)' : 'var(--ui-muted)';
    };
    if (!MD.fsx.isApp) {
      fmtAssoc(false);
      U.$('#se-assoc', body).disabled = true;
    } else {
      MD.fsx.mdAssocStatus().then(fmtAssoc).catch(() => fmtAssoc(false));
      U.$('#se-assoc', body).addEventListener('click', async () => {
        try {
          await MD.fsx.registerMdAssoc();
          fmtAssoc(true);
          MD.toast('已登记，正在打开 Windows 默认应用设置…', 3500);
          await MD.fsx.openDefaultAppsSettings();
        } catch (e) {
          MD.toast('关联失败：' + (e && e.message ? e.message : e));
        }
      });
    }
  }

  function showHelp() {
    const body = U.$('#help-body');
    const rows = [
      ['Ctrl+S', '保存'], ['Ctrl+N', '新建文档'], ['Ctrl+O', '打开文件（可多选）'],
      ['Ctrl+Shift+O', '打开文件夹'], ['Ctrl+Alt+O', '导入 PDF/Word 等为 Markdown'],
      ['Ctrl+Tab / Ctrl+PageDown', '切换到下一个标签页'],
      ['Ctrl+Shift+Tab / Ctrl+PageUp', '切换到上一个标签页'],
      ['Ctrl+W', '关闭当前标签页'],
      ['Ctrl+B / Ctrl+I', '加粗 / 斜体'],
      ['Ctrl+K', '插入链接'], ['Ctrl+1..4', '一/二/三/四级标题'],
      ['Ctrl+Shift+Q', '引用'], ['Ctrl+F / Ctrl+H', '查找 / 替换（编辑器内）'],
      ['Alt+1..4', '源码 / 分栏 / 预览 / 阅读模式'], ['Ctrl+\\', '显示/隐藏侧栏'],
      ['Esc', '退出阅读模式 / 关闭面板'],
      ['Ctrl+P', '导出 PDF（直接落盘，无页眉页脚）'], ['Ctrl+= / Ctrl+-', '编辑器字号增减'],
      ['Ctrl+滚轮', '放大 / 缩小画面'], ['Ctrl+0', '画面缩放回到 100%'],
      ['F11', '全屏'],
    ];
    body.innerHTML = rows.map(([k, v]) => `
      <div class="ctl-row"><label style="width:150px"><kbd style="margin:0">${k}</kbd></label><span>${v}</span></div>
    `).join('');
    U.$('#help-modal').hidden = false;
  }

  /* ── 对外接口 ── */
  MD.theme = {
    BUILTIN, PAPERS, getTheme, allThemes, isDark,
    applyUI, applyDoc, resolvedVars,
    renderThemeMenu, renderLayoutPanel, renderSettings,
    openThemeEditor, closeThemeEditor, clearDraftPreview, showHelp,
    importTheme,
    FONTS_BODY, FONTS_HEADING, FONTS_MONO,
  };
})();
