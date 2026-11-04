/* ══════════════ editor.js：CodeMirror 编辑器封装与格式化命令 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  let cm = null;
  let typewriter = false;
  // 整体替换（set value）期间抑制 change 事件，避免误触发自动保存把旧文件写回
  let _settingValue = false;

  function init(hostEl) {
    cm = CodeMirror(hostEl, {
      value: '',
      mode: { name: 'markdown', fencedCodeBlockHighlighting: false },
      lineNumbers: true,
      lineWrapping: true,
      autofocus: false,
      inputStyle: 'textarea',
      indentUnit: 2,
      tabSize: 2,
      dragDrop: false,
      autoCloseBrackets: true,
      styleActiveLine: { nonEmpty: true },
      matchBrackets: true,
      highlightSelectionMatches: { showToken: false, annotateScrollbar: true },
      extraKeys: {
        'Enter': 'newlineAndIndentContinueMarkdownList',
        'Ctrl-B': () => actions.bold(),
        'Cmd-B': () => actions.bold(),
        'Ctrl-I': () => actions.italic(),
        'Cmd-I': () => actions.italic(),
        'Ctrl-K': () => actions.link(),
        'Cmd-K': () => actions.link(),
        'Ctrl-1': () => actions.heading(1),
        'Ctrl-2': () => actions.heading(2),
        'Ctrl-3': () => actions.heading(3),
        'Ctrl-4': () => actions.heading(4),
        'Ctrl-Shift-Q': () => actions.quote(),
        'Ctrl-F': 'findPersistent',
        'Cmd-F': 'findPersistent',
        'Ctrl-H': 'replace',
        'Shift-Ctrl-F': 'replace',
        'Tab': (c) => {
          if (c.somethingSelected()) c.execCommand('indentMore');
          else c.replaceSelection('  ');
        },
        'Shift-Tab': (c) => c.execCommand('indentLess'),
        'Ctrl-=': () => MD.app.changeEditorFontSize(1),
        'Ctrl--': () => MD.app.changeEditorFontSize(-1),
      },
    });

    cm.on('change', (_c, changeObj) => {
      // setValue 的整体替换不算用户编辑：否则打开/新建/关闭后紧跟的 markDirty(false)
      // 会被 1.5 秒后的自动保存推翻，把上一个文件的内容写回当前文件
      if (_settingValue || changeObj.origin === 'setValue') return;
      MD.bus.emit('doc:changed');
    });
    cm.on('cursorActivity', () => {
      if (typewriter) {
        const scroller = cm.getScrollerElement();
        const top = cm.cursorCoords(null, 'local').top;
        cm.scrollTo(null, Math.max(0, top - scroller.clientHeight / 2));
      }
      MD.bus.emit('cursor:moved', cm.getCursor());
    });
    cm.on('paste', (instance, ev) => {
      const cd = ev.clipboardData;
      if (!cd) return;
      for (const item of (cd.items || [])) {
        if (item.type && item.type.startsWith('image/')) {
          const file = item.getAsFile();
          if (file) {
            ev.preventDefault();
            MD.actions.handleImageFile(file);
            return;
          }
        }
      }
    });

    // 拖拽：图片 → 插入，md 文件 → 打开
    cm.getWrapperElement().addEventListener('dragover', (e) => { e.preventDefault(); e.stopPropagation(); });
    cm.getWrapperElement().addEventListener('drop', (e) => {
      e.preventDefault(); e.stopPropagation();
      MD.actions.handleDrop(e.dataTransfer);
    });

    return cm;
  }

  /* ── 选区工具 ── */
  function wrapSelection(before, after, placeholder) {
    const sel = cm.getSelection();
    if (sel) {
      // 先记下替换前选区的起止（anchor/head 按位置排序），替换后据此重新选中内容；
      // 旧写法用替换后光标反推列号，多行选区会算出负数列
      const r = cm.listSelections()[0];
      const a = r.anchor, h = r.head;
      const aFirst = a.line < h.line || (a.line === h.line && a.ch <= h.ch);
      const from = aFirst ? { line: a.line, ch: a.ch } : { line: h.line, ch: h.ch };
      const to = aFirst ? { line: h.line, ch: h.ch } : { line: a.line, ch: a.ch };
      cm.replaceSelection(before + sel + after);
      // 多行时终点列用替换后该行的实际长度兜底，确保不为负、不越界
      const endCh = Math.min(to.ch + before.length, (cm.getLine(to.line) || '').length);
      cm.setSelection({ line: from.line, ch: from.ch + before.length }, { line: to.line, ch: endCh });
    } else {
      const cur = cm.getCursor();
      cm.replaceRange(before + (placeholder || '') + after, cur, cur);
      cm.setSelection({ line: cur.line, ch: cur.ch + before.length }, { line: cur.line, ch: cur.ch + before.length + (placeholder || '').length });
    }
    cm.focus();
  }
  function toggleLinePrefix(prefix, regexEscape) {
    const ranges = cm.listSelections();
    const startLine = Math.min(...ranges.map((r) => r.from().line));
    const endLine = Math.max(...ranges.map((r) => r.to().line));
    const re = new RegExp('^\\s*' + (regexEscape || prefix.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
    for (let ln = startLine; ln <= endLine; ln++) {
      const text = cm.getLine(ln);
      if (text == null) continue;
      if (re.test(text)) {
        cm.replaceRange(text.replace(re, ''), { line: ln, ch: 0 }, { line: ln, ch: text.match(re)[0].length });
      } else {
        cm.replaceRange(prefix, { line: ln, ch: 0 }, { line: ln, ch: 0 });
      }
    }
    cm.focus();
  }
  function insertBlock(text) {
    const cur = cm.getCursor();
    const line = cm.getLine(cur.line) || '';
    const needNl = line.trim() !== '' ? 1 : 0;
    cm.replaceRange('\n'.repeat(needNl) + text + '\n', { line: cur.line, ch: line.length });
    cm.focus();
  }
  function setHeading(level) {
    const line = cm.getCursor().line;
    const text = cm.getLine(line) || '';
    const stripped = text.replace(/^\s*#{1,6}\s+/, '');
    if (level === 0) cm.replaceRange(stripped, { line, ch: 0 }, { line, ch: text.length });
    else cm.replaceRange('#'.repeat(level) + ' ' + stripped, { line, ch: 0 }, { line, ch: text.length });
    cm.focus();
  }

  const actions = {
    bold: () => wrapSelection('**', '**', '加粗文本'),
    italic: () => wrapSelection('*', '*', '斜体文本'),
    strike: () => wrapSelection('~~', '~~', '删除线'),
    code: () => wrapSelection('`', '`', '代码'),
    inlineMath: () => wrapSelection('$', '$', 'E=mc^2'),
    heading: (l) => setHeading(l),
    quote: () => toggleLinePrefix('> '),
    ul: () => toggleLinePrefix('- '),
    ol: () => toggleLinePrefix('1. '),
    task: () => toggleLinePrefix('- [ ] '),
    hr: () => insertBlock('---'),
    link: () => wrapSelection('[', '](https://)', '链接文字'),
    imageLink: (path, alt) => {
      const cur = cm.getCursor();
      cm.replaceRange(`![${alt || ''}](${path})`, cur, cur);
      cm.focus();
    },
    codeBlock: (lang) => insertBlock('```' + (lang || '') + '\n\n```'),
    table: (rows = 3, cols = 3) => {
      const head = '| ' + Array.from({ length: cols }, (_, i) => '列' + (i + 1)).join(' | ') + ' |';
      const sep = '| ' + Array.from({ length: cols }, () => '---').join(' | ') + ' |';
      const body = Array.from({ length: rows - 1 }, () => '| ' + Array.from({ length: cols }, () => '  ').join(' | ') + ' |');
      insertBlock([head, sep, ...body].join('\n'));
    },
  };

  /* ── 从预览切换任务勾选 ── */
  // 与 MD.md.taskLines 共用同一套匹配：跳过代码围栏，允许 > 引用、有序/无序前缀
  const TASK_LINE_RE = /^(\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[)([ xX])(\])/;
  /** 按源码行号回写任务勾选状态（行号来自统一扫描），避免按「第 N 个」错位 */
  function setTaskByLine(lineNo, checked) {
    const text = cm.getLine(lineNo);
    if (text == null) return false;
    const m = TASK_LINE_RE.exec(text);
    if (!m) return false;
    const next = m[1] + (checked ? 'x' : ' ') + m[3] + text.slice(m[0].length);
    cm.replaceRange(next, { line: lineNo, ch: 0 }, { line: lineNo, ch: text.length }, { origin: '+task' });
    return true;
  }
  /** 兼容包装：第 index 个任务项 → 源码行号 → 按行回写 */
  function setTaskByIndex(index, checked) {
    const lineNo = MD.md.taskLines(cm.getValue())[index];
    if (lineNo == null) return false;
    return setTaskByLine(lineNo, checked);
  }

  MD.editor = {
    init,
    get cm() { return cm; },
    get value() { return cm ? cm.getValue() : ''; },
    set value(v) {
      if (!cm) return;
      _settingValue = true;
      try { cm.setValue(v); } finally { _settingValue = false; }
      // 打开/新建/关闭后清掉撤销历史，避免 Ctrl+Z 把上一个文件内容写回当前文件
      cm.clearHistory();
      cm.markClean();
    },
    focus: () => cm && cm.focus(),
    actions,
    setTaskByIndex, setTaskByLine,
    setTypewriter(on) { typewriter = !!on; },
    getTypewriter: () => typewriter,
    scrollToLine(line) {
      if (!cm) return;
      cm.setCursor({ line, ch: 0 });
      cm.scrollIntoView({ line, ch: 0 }, 120);
      cm.focus();
    },
    getCursorPosition() { return cm ? cm.getCursor() : { line: 0, ch: 0 }; },
    changeFontSize(delta) {
      const s = parseFloat(getComputedStyle(cm.getWrapperElement()).fontSize);
      const next = U.clamp(Math.round((s + delta) * 2) / 2, 10, 28);
      cm.getWrapperElement().style.fontSize = next + 'px';
      cm.refresh();
      MD.settings.fontSize = next;
      MD.saveSettings();
      return next;
    },
    applyFontSize(px) {
      if (!cm) return;
      cm.getWrapperElement().style.fontSize = px + 'px';
      cm.refresh();
    },
  };
})();
