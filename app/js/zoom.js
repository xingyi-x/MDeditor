/* ══════════════ zoom.js：Ctrl + 滚轮 / 快捷键 缩放整个画面 ══════════════ */
'use strict';

(function () {
  const U = MD.util;
  const MIN = 50;
  const MAX = 250;
  const STEP = 5;

  let cur = 100;          // 当前缩放百分比
  let acc = 0;            // 滚轮累积量（一档 = 5%）
  let native = false;     // 是否走 WebView 原生缩放
  let probed = false;     // 是否已探测过原生缩放能力
  let flashTimer = null;
  let chain = Promise.resolve();

  function clampPct(v) {
    const n = Math.round(Number(v) / STEP) * STEP;
    if (!isFinite(n)) return 100;
    return U.clamp(n, MIN, MAX);
  }

  /* 原生缩放优先：WebView2 的 zoom 是浏览器级缩放，不改变 CSS 布局坐标系，
     CodeMirror 光标测量、分栏拖拽比例、paged.js 的纸张尺寸都不会错位。
     CSS zoom 只在浏览器调试（无 Tauri）时降级使用。 */
  function tauriSetZoom(zf) {
    const W = window.__TAURI__ && window.__TAURI__.webview;
    if (!W || typeof W.getCurrentWebview !== 'function') return null;
    let wv;
    try { wv = W.getCurrentWebview(); } catch (e) { return null; }
    if (!wv || typeof wv.setZoom !== 'function') return null;
    try {
      const p = wv.setZoom(zf);
      if (p && typeof p.then === 'function') return p.then(() => true).catch(() => false);
      return Promise.resolve(true);
    } catch (e) { return Promise.resolve(false); }
  }

  function cssZoom(zf) {
    const html = document.documentElement;
    if (zf === 1) {
      html.style.zoom = '';
      html.style.width = '';
      html.style.height = '';
    } else {
      html.style.zoom = String(zf);
      // html 的 100% 尺寸按未缩放坐标计算，zoom 之后会溢出窗口 → 反向补偿
      html.style.width = 'calc(100vw / ' + zf + ')';
      html.style.height = 'calc(100vh / ' + zf + ')';
    }
    if (MD.editor && MD.editor.cm) { try { MD.editor.cm.refresh(); } catch (e) { /* 忽略 */ } }
  }

  function paint() {
    const el = document.getElementById('st-zoom');
    if (!el) return;
    el.textContent = cur + '%';
    el.title = '画面缩放 ' + cur + '%（Ctrl+滚轮调整，点击恢复 100%）';
    el.classList.toggle('st-zoom-hot', cur !== 100);
  }

  async function apply(z) {
    const zf = z / 100;
    // 顶栏精简标记：原生缩放会改变视口宽度触发媒体查询，CSS 降级不会，靠这个标记对齐
    document.documentElement.dataset.zoom = z >= 120 ? 'large' : (z <= 85 ? 'small' : '');
    if (!probed || native) {
      const r = await tauriSetZoom(zf);
      if (r === true) { native = true; probed = true; }
      else if (r === false || r === null) { native = false; probed = true; }
    }
    if (!native) cssZoom(zf);
    paint();
  }

  function flash() {
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => {
      if (MD.toast) MD.toast('画面缩放 ' + cur + '%', 1000);
    }, 180);
  }

  /* 串行化：滚轮一次可能连发多档，必须保证按顺序落地，否则最终值会乱。
     cur 必须在这里同步更新——若等到微任务里再改，同一帧内的连续滚轮都会读到旧值，
     滚得再快也只会缩放一档。 */
  function set(pct, opts) {
    const o = opts || {};
    const z = clampPct(pct);
    cur = z;
    chain = chain
      .then(() => apply(z))
      .then(() => {
        if (o.persist !== false && MD.settings) {
          MD.settings.zoom = z;
          MD.saveSettings();
        }
        if (!o.silent) flash();
        return z;
      })
      .catch(() => cur);
    return chain;
  }

  /* ── 滚轮：Ctrl / Cmd + 滚轮 ── */
  function onWheel(e) {
    if (!(e.ctrlKey || e.metaKey)) return;
    e.preventDefault();
    e.stopPropagation();
    let d = e.deltaY;
    if (e.deltaMode === 1) d *= 16;        // 行
    else if (e.deltaMode === 2) d *= 400;  // 页
    if (!d) return;
    acc += d;
    let dir = 0;
    while (Math.abs(acc) >= 100) {
      dir += acc < 0 ? 1 : -1;
      acc -= (acc < 0 ? -1 : 1) * 100;
    }
    if (dir) set(cur + dir * STEP);
  }

  /* ── 键盘：Ctrl+0 回到 100%。
       不用 Ctrl+= / Ctrl+-：那两个键在编辑器里已经代表「编辑器字号增减」（CodeMirror extraKeys），
       抢占会改变用户已有的操作习惯。 */
  function onKey(e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    if (e.key === '0') { e.preventDefault(); set(100); }
  }

  function init(pct) {
    cur = clampPct(pct);
    apply(cur);
    const el = document.getElementById('st-zoom');
    if (el) {
      el.addEventListener('click', () => set(100));
    }
    document.addEventListener('wheel', onWheel, { passive: false, capture: true });
    document.addEventListener('keydown', onKey, true);
  }

  MD.zoom = {
    init,
    get: () => cur,
    set,
    reset: () => set(100),
    min: MIN,
    max: MAX,
    step: STEP,
    isNative: () => native,
  };
})();
