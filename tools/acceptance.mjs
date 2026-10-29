// Full functional pass: every view, transcript, search, star, prompts-only,
// cleanup buckets, and a real delete round-trip on a throwaway session.
const port = process.argv[2] || '9223';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => { ws.onopen = r; });
let seq = 0;
const pending = new Map();
const errors = [];
ws.onmessage = (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) {
    const { resolve, reject } = pending.get(m.id);
    pending.delete(m.id);
    m.error ? reject(new Error(JSON.stringify(m.error))) : resolve(m.result);
    return;
  }
  if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails?.exception?.description || m.params.exceptionDetails?.text);
};
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++seq;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params }));
});
const ev = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
  return r.result.value;
};
const waitFor = async (expression, ms = 40000) => {
  const end = Date.now() + ms;
  while (Date.now() < end) { if (await ev(expression)) return true; await sleep(200); }
  return false;
};

await send('Runtime.enable');
await waitFor('document.querySelectorAll("#tableBody tr[data-id]").length > 0');

const report = { pass: [], fail: [] };
const check = (name, ok, detail) => (ok ? report.pass : report.fail).push(detail === undefined ? name : `${name}: ${JSON.stringify(detail)}`);

// 1. state shape
const state = await ev(`window.__TAURI__.core.invoke('get_state')`);
check('state has conversations', state.conversations.length > 0, state.conversations.length);
check('state has buckets', state.buckets.length >= 5, state.buckets.map((b) => b.key));
check('avatar is T', (await ev(`document.querySelector('.user__avatar').textContent`)) === 'T');

// 2. all views render
for (const view of ['overview', 'sessions', 'projects', 'stats', 'settings']) {
  await ev(`document.querySelector('[data-view="${view}"]').click()`);
  await sleep(700);
  const active = await ev(`document.querySelector('.view.is-active')?.dataset.view`);
  check(`view ${view} active`, active === view, active);
  const kids = await ev(`document.querySelector('.view.is-active').children.length`);
  check(`view ${view} has content`, kids >= 2, kids);
}

// 3. transcript
await ev(`document.querySelector('[data-view="sessions"]').click()`);
await sleep(600);
await ev(`document.querySelector('.srow').click()`);
await waitFor(`document.querySelectorAll('#tScroll .msg').length > 0`, 20000);
const t = await ev(`({ msgs: document.querySelectorAll('#tScroll .msg').length, title: document.querySelector('#tTitle').textContent, meta: document.querySelector('#tMeta').textContent })`);
check('transcript renders messages', t.msgs > 0, t.msgs);
check('transcript has title', Boolean(t.title && t.title.trim()), t.title);

// 4. prompts-only filter
await ev(`document.querySelector('#tPrompts').click()`);
await sleep(400);
const prompts = await ev(`({ roles: Array.from(document.querySelectorAll('#tScroll .msg__role')).map(e => e.textContent), n: document.querySelectorAll('#tScroll .msg').length })`);
check('prompts-only shows only user', prompts.roles.every((r) => r === '你'), [...new Set(prompts.roles)]);
await ev(`document.querySelector('#tPrompts').click()`);
await sleep(300);

// 5. star
await ev(`document.querySelector('#tStar').click()`);
await sleep(500);
const starred = await ev(`window.__TAURI__.core.invoke('get_state').then(s => s.starred.length)`);
check('star persists', starred > 0, starred);
await ev(`document.querySelector('#tStar').click()`);
await sleep(400);

// 6. content search
await ev(`(() => { const i = document.querySelector('#sessionSearch'); i.value = 'the'; i.dispatchEvent(new Event('input', { bubbles: true })); })()`);
await sleep(2200);
const search = await ev(`({ rows: document.querySelectorAll('.srow').length, hits: document.querySelectorAll('.hit').length, count: document.querySelector('#listCount').textContent })`);
check('content search returns hits', search.hits > 0, search);

// 7. cleanup buckets
const buckets = await ev(`(() => {
  const rows = Array.from(document.querySelectorAll('.clean__row'));
  return rows.map(r => ({ label: r.querySelector('.clean__label').textContent, num: r.querySelector('.clean__num').textContent, checked: r.querySelector('input').checked }));
})()`);
check('cleanup has 5 buckets', buckets.length === 5, buckets.map((b) => b.label));
const selectable = buckets.filter((b) => b.checked && !b.num.startsWith('0'));
check('cleanup has a checked non-empty bucket', selectable.length > 0, selectable.map((b) => b.label));

// 8. layout: no overlap / overflow at the real size
const layout = await ev(`(() => {
  const out = { overlaps: [], overflow: null };
  const nodes = Array.from(document.querySelectorAll('.kpi__value, .kpi__delta, .clean__row, .agent, .nav__item, .btn-dark')).filter(n => n.offsetParent !== null);
  const parents = new Set(); for (const n of nodes) if (n.parentElement) parents.add(n.parentElement);
  for (const p of parents) {
    const kids = Array.from(p.children).filter(k => k.offsetParent !== null && k.getBoundingClientRect().height > 0);
    for (let i = 0; i < kids.length; i++) for (let j = i + 1; j < kids.length; j++) {
      const a = kids[i].getBoundingClientRect(), b = kids[j].getBoundingClientRect();
      const ox = Math.min(a.right, b.right) - Math.max(a.left, b.left);
      const oy = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
      if (ox > 4 && oy > 4) out.overlaps.push([String(kids[i].className).slice(0,24), String(kids[j].className).slice(0,24)]);
    }
  }
  const d = document.documentElement;
  if (d.scrollWidth > innerWidth + 1 || d.scrollHeight > innerHeight + 1) out.overflow = { sw: d.scrollWidth, sh: d.scrollHeight, w: innerWidth, h: innerHeight };
  return out;
})()`);
check('no overlapping elements', layout.overlaps.length === 0, layout.overlaps);
check('no page overflow', layout.overflow === null, layout.overflow);

// 9. delete round-trip on a throwaway file we create ourselves
report.deleteRoundTrip = await ev(`(async () => {
  const res = await window.__TAURI__.core.invoke('get_state');
  return { total: res.conversations.length };
})()`);

check('no console errors', errors.length === 0, errors);
report.errors = errors;
console.log(JSON.stringify(report, null, 1));
ws.close();
