/* ══════════════ editor.js：CodeMirror 编辑器封装与格式化命令 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  let cm = null;
  let typewriter = false;

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
      if (changeObj.origin === 'setValue') return;
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
      cm.replaceSelection(before + sel + after);
      cm.setSelection({ line: cm.getCursor().line, ch: cm.getCursor().ch - after.length - sel.length },
        { line: cm.getCursor().line, ch: cm.getCursor().ch - after.length });
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
  function setTaskByIndex(index, checked) {
    const text = cm.getValue();
    let n = -1;
    const lines = text.split('\n');
    for (let i = 0; i < lines.length; i++) {
      const m = /^(\s*[-*+]\s+\[)([ xX])(\])/.exec(lines[i]);
      if (m) {
        n++;
        if (n === index) {
          lines[i] = m[1] + (checked ? 'x' : ' ') + m[3];
          const from = { line: i, ch: 0 };
          const to = { line: i, ch: cm.getLine(i).length };
          cm.replaceRange(lines[i], from, to, { origin: '+task' });
          return true;
        }
      }
    }
    return false;
  }

  MD.editor = {
    init,
    get cm() { return cm; },
    get value() { return cm ? cm.getValue() : ''; },
    set value(v) { if (cm) cm.setValue(v); },
    focus: () => cm && cm.focus(),
    actions,
    setTaskByIndex,
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
