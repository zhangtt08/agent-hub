const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

const S = {
  data: null,
  token: '',
  scanning: false,
  indexing: false,
  tab: 'all',
  query: '',
  agent: 'all',
  status: 'all',
  selected: new Set(),
  // Nothing is pre-selected: a cleanup click must never delete something the
  // user did not explicitly tick.
  checkedBuckets: new Set(),
  activeId: null,
  previews: new Map(),
  previewLoading: false,
  autoSelected: false,
  pollTimer: null,
  // Always start in the recoverable mode. "永久删除" has to be chosen again each
  // time, so a stale preference can never make the next click irreversible.
  deleteMode: 'recycle',
};

const AGENT_LOGO = {
  codex: 'openai', claude: 'claude', gemini: 'gemini', qwen: 'qwen', continue: 'continue', aider: 'aider',
  dsh: 'deepseek', workbuddy: 'workbuddy', opencode: 'opencode', antigravity: 'gemini',
  trae: 'trae', qoder: 'qoder', zcode: 'zcode',
};
const AGENT_TINT = {
  codex: '#0F7A56', claude: '#BE5B33', gemini: '#4A64C4', qwen: '#6A4FA0', continue: '#2E6FB7', aider: '#DA6F35',
  dsh: '#4E6BD8', workbuddy: '#2E6FB7', opencode: '#7A4FA0', antigravity: '#4A64C4',
  trae: '#DA6F35', qoder: '#B5482F', zcode: '#1B1815',
};
const AGENT_TINT_SOFT = {
  codex: '#E4F2E7', claude: '#F3E7DA', gemini: '#E8EDFB', qwen: '#F1E9F8', continue: '#E6EEF8', aider: '#F8EADF',
  dsh: '#E7EBFA', workbuddy: '#E6EEF8', opencode: '#F1E9F8', antigravity: '#E8EDFB',
  trae: '#F8EADF', qoder: '#F8E4DD', zcode: '#ECEAE3',
};

const TAURI = typeof window !== 'undefined' && window.__TAURI__ ? window.__TAURI__ : null;
const BRIDGE = TAURI ? {
  state: () => TAURI.core.invoke('get_state'),
  rescan: () => TAURI.core.invoke('rescan'),
  archive: (id, archived) => TAURI.core.invoke('archive', { id, archived }),
  remove: (ids, purge) => TAURI.core.invoke('delete_sessions', { ids, purge }),
  forceRemove: (ids) => TAURI.core.invoke('force_delete', { ids }),
  preview: (id) => TAURI.core.invoke('preview_conversation', { id }),
  transcript: (id) => TAURI.core.invoke('get_transcript', { id }),
  search: (q) => TAURI.core.invoke('search_text', { q }),
  star: (id, starred) => TAURI.core.invoke('star', { id, starred }),
  exportMd: (name, content) => TAURI.core.invoke('export_markdown', { name, content }),
  reveal: (id) => TAURI.core.invoke('reveal', { id }),
  revealData: () => TAURI.core.invoke('reveal_data'),
} : null;

/* ---------------------------------------------------------------- utils */

function esc(value) {
  return String(value == null ? '' : value)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function fmtBytes(n) {
  if (!n) return '0 B';
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(2) + ' GB';
  if (n >= 1 << 20) return (n / (1 << 20)).toFixed(1) + ' MB';
  if (n >= 1024) return Math.round(n / 1024) + ' KB';
  return n + ' B';
}

function fmtBytesParts(n) {
  if (!n) return ['0', 'B'];
  if (n >= 1 << 30) return [(n / (1 << 30)).toFixed(2), 'GB'];
  if (n >= 1 << 20) return [(n / (1 << 20)).toFixed(1), 'MB'];
  if (n >= 1024) return [Math.round(n / 1024), 'KB'];
  return [n, 'B'];
}

function fmtNum(n) {
  return Number(n || 0).toLocaleString('en-US');
}

function relTime(iso) {
  const t = Date.parse(iso);
  if (!t) return '—';
  const diff = Date.now() - t;
  if (diff < 60e3) return '刚刚';
  if (diff < 3600e3) return Math.floor(diff / 60e3) + ' 分钟前';
  if (diff < 86400e3) return Math.floor(diff / 3600e3) + ' 小时前';
  if (diff < 7 * 86400e3) return Math.floor(diff / 86400e3) + ' 天前';
  const d = new Date(t);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

function fmtClock(iso) {
  if (!iso) return '';
  const t = Date.parse(iso);
  if (!t) return '';
  const d = new Date(t);
  return `${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

function agentLogo(id) {
  return `<svg><use href="#logo-${AGENT_LOGO[id] || 'qwen'}"/></svg>`;
}

function tintStyle(id) {
  return `background:${AGENT_TINT_SOFT[id] || '#F1EADF'};color:${AGENT_TINT[id] || '#6E665A'}`;
}

function toast(text, kind = 'ok') {
  const el = document.createElement('div');
  el.className = 'toast' + (kind === 'error' ? ' toast--error' : '');
  el.innerHTML = `<svg><use href="#i-${kind === 'error' ? 'warning' : 'check'}"/></svg><span>${esc(text)}</span>`;
  $('#toasts').appendChild(el);
  setTimeout(() => el.remove(), 4200);
}

function errText(err) {
  if (!err) return '操作失败';
  if (typeof err === 'string') return err;
  return err.message || JSON.stringify(err);
}

async function api(path, options = {}) {
  const opts = { ...options, headers: { ...(options.headers || {}) } };
  if (opts.body) {
    opts.headers['content-type'] = 'application/json';
    opts.body = JSON.stringify(opts.body);
  }
  if (options.method === 'POST') opts.headers['x-dsp-token'] = S.token;
  const res = await fetch(path, opts);
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `请求失败（${res.status}）`);
  return data;
}

/* ---------------------------------------------------------------- state */

async function load({ silent = false } = {}) {
  const data = BRIDGE ? await BRIDGE.state() : await api('/api/state');
  S.data = data;
  window.__DATA__ = data;
  S.token = data.token || '';
  VS.stars = new Set(data.starred || []);
  VS.starredOnly = VS.starredOnly && VS.stars.size > 0;
  S.scanning = Boolean(data.scanning && data.scanning.running);
  S.indexing = Boolean(data.indexing);
  const ids = new Set(data.conversations.map((c) => c.id));
  for (const id of Array.from(S.selected)) if (!ids.has(id)) S.selected.delete(id);
  if (S.activeId && !ids.has(S.activeId)) S.activeId = null;
  if (!silent) renderAll();
  // Keep polling while the background index builds so the status clears itself.
  if (S.scanning || S.indexing) schedulePoll();
  return data;
}

// The first scan finishes after the window opens, so the initial selection has
// to happen when rows actually arrive (not on first load).
function ensureSelection() {
  if (S.autoSelected) return;
  const list = conversations();
  if (!list.length) return;
  S.autoSelected = true;
  if (!S.activeId) {
    const first = list.find((c) => c.previewable) || list[0];
    S.activeId = first.id;
  }
}

function schedulePoll() {
  clearTimeout(S.pollTimer);
  S.pollTimer = setTimeout(async () => {
    try {
      const data = BRIDGE ? await BRIDGE.state() : await api('/api/state');
      S.data = data;
      S.scanning = data.scanning && data.scanning.running;
      S.indexing = Boolean(data.indexing);
      renderScanState();
      renderTable();
      renderAgents();
      if (S.scanning || S.indexing) schedulePoll();
      else renderAll();
    } catch {
      S.scanning = false;
    }
  }, 700);
}

function conversations() {
  return (S.data && S.data.conversations) || [];
}

function visibleRows() {
  const q = S.query.trim().toLowerCase();
  const cutoff = Date.now() - 7 * 86400000;
  return conversations().filter((c) => {
    if (S.tab === 'recent' && Date.parse(c.mtime) < cutoff) return false;
    if (S.tab === 'archived' && !c.archived) return false;
    if (S.agent !== 'all' && c.agent !== S.agent) return false;
    if (S.status === 'archived' && !c.archived) return false;
    if (S.status === 'active' && c.archived) return false;
    if (S.status === 'large' && c.size <= 20 * 1024 * 1024) return false;
    if (q) {
      const hay = `${c.title} ${c.project} ${c.path} ${c.agent}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
}

function bucketUnion() {
  const buckets = (S.data && S.data.buckets) || [];
  const ids = new Set();
  let bytes = 0;
  for (const b of buckets) {
    if (!S.checkedBuckets.has(b.key)) continue;
    for (const id of b.ids) ids.add(id);
    bytes += b.bytes;
  }
  return { ids: Array.from(ids), bytes };
}

/* ---------------------------------------------------------------- render */

function renderAll() {
  ensureSelection();
  renderScanState();
  renderAgentOptions();
  renderKpis();
  renderAgents();
  renderBuckets();
  renderTable();
  renderPreview();
  renderStorage();
  if (VS.current !== 'overview') renderActiveView();
}

function renderAgentOptions() {
  const agents = (S.data.agents || []).filter((a) => a.count > 0);
  const options = `<option value="all">全部 Agent</option>` +
    agents.map((a) => `<option value="${esc(a.id)}">${esc(a.name)}</option>`).join('');
  const select = $('#agentFilter');
  if (select) {
    const current = S.agent;
    select.innerHTML = options;
    select.value = current;
  }
  const sessionSelect = $('#sessionAgentFilter');
  if (sessionSelect) {
    sessionSelect.innerHTML = options;
    sessionSelect.value = VS.agent;
  }
}

function renderScanState() {
  const d = S.data;
  if (!d) return;
  const when = d.scannedAt ? new Date(d.scannedAt) : null;
  if (S.scanning) {
    const p = d.scanning || {};
    $('#scanMeta').textContent = `正在扫描本机 Agent 数据… ${p.done || 0}/${p.total || '?'}`;
  } else if (when) {
    const secs = d.durationMs ? ` · 用时 ${(d.durationMs / 1000).toFixed(1)}s` : '';
    // 索引在后台建立；期间正文搜索只能命中已索引的部分，需要让用户看见。
    const idx = d.indexing ? ' · 正在建立内容索引…' : '';
    $('#scanMeta').textContent = `上次扫描 ${when.toLocaleString('zh-CN', { hour12: false })}${secs}${idx}`;
  } else {
    $('#scanMeta').textContent = '尚未扫描';
  }
  $('#rescanBtn').disabled = S.scanning;
  const firstRun = !(S.data.conversations || []).length;
  $('#busy').hidden = !(S.scanning && firstRun);
  if (S.scanning && firstRun) $('#busyText').textContent = '正在读取本机 Agent 会话，请稍候…';
}

function renderKpis() {
  const t = S.data.totals;
  const union = bucketUnion();
  const [sizeVal, sizeUnit] = fmtBytesParts(t.bytes);
  const installed = t.installed;

  $('#kpis').innerHTML = `
    <article class="kpi kpi--yellow">
      <div class="kpi__top"><span class="kpi__icon"><svg><use href="#i-chat"/></svg></span><span>已识别 Agent</span></div>
      <div class="kpi__value num">${installed}</div>
      <div class="kpi__delta"><span class="up">${t.withSessions} 个</span><span class="note">有会话记录</span></div>
      <svg class="kpi__art" width="150" height="70" viewBox="0 0 150 70" fill="none" aria-hidden="true">
        <rect x="16" y="40" width="12" height="30" rx="4" fill="#161310" opacity=".10"/>
        <rect x="36" y="28" width="12" height="42" rx="4" fill="#161310" opacity=".13"/>
        <rect x="56" y="36" width="12" height="34" rx="4" fill="#161310" opacity=".10"/>
        <rect x="76" y="18" width="12" height="52" rx="4" fill="#161310" opacity=".16"/>
        <rect x="96" y="30" width="12" height="40" rx="4" fill="#161310" opacity=".11"/>
        <rect x="116" y="10" width="12" height="60" rx="4" fill="#161310" opacity=".18"/>
      </svg>
    </article>

    <article class="kpi kpi--orange">
      <div class="kpi__top"><span class="kpi__icon"><svg><use href="#i-file"/></svg></span><span>会话文件</span></div>
      <div class="kpi__value num">${fmtNum(t.conversations)}</div>
      <div class="kpi__delta"><span class="up">${fmtNum(t.messages)}</span><span class="note">条消息</span></div>
      <svg class="kpi__art" width="240" height="86" viewBox="0 0 240 86" preserveAspectRatio="none" aria-hidden="true">
        <path d="M0 54c26-26 46 10 70-4s38-22 62-10 44 30 68 12 22-16 40-10v44H0Z" fill="#C95F26" opacity=".38"/>
        <path d="M0 70c30-18 52 8 78-2s44-16 66-6 40 20 62 8 20-10 34-6v22H0Z" fill="#B9521E" opacity=".28"/>
      </svg>
    </article>

    <article class="kpi kpi--cream">
      <div class="kpi__top"><span class="kpi__icon"><svg><use href="#i-folder"/></svg></span><span>占用空间</span></div>
      <div class="kpi__value num">${sizeVal} <small>${sizeUnit}</small></div>
      <div class="kpi__delta"><span class="note">${fmtNum(t.conversations)} 个文件</span></div>
      <svg class="kpi__art" width="150" height="66" viewBox="0 0 150 66" fill="none" aria-hidden="true">
        <rect x="20" y="30" width="9" height="36" rx="4" fill="#161310" opacity=".07"/>
        <rect x="36" y="22" width="9" height="44" rx="4" fill="#161310" opacity=".09"/>
        <rect x="52" y="34" width="9" height="32" rx="4" fill="#161310" opacity=".06"/>
        <rect x="68" y="14" width="9" height="52" rx="4" fill="#161310" opacity=".10"/>
        <rect x="84" y="26" width="9" height="40" rx="4" fill="#161310" opacity=".07"/>
      </svg>
    </article>

    <article class="kpi kpi--black">
      <div class="kpi__top"><span class="kpi__icon"><svg><use href="#i-trash"/></svg></span><span>可清理</span></div>
      <div class="kpi__value num">${fmtNum(union.ids.length)} <small>个</small></div>
      <div class="kpi__delta"><span class="note">预计释放 ${fmtBytes(union.bytes)}</span></div>
      <button class="kpi__cta" type="button" id="jumpClean">立即清理 <svg><use href="#i-arrow"/></svg></button>
    </article>`;

  $('#jumpClean').addEventListener('click', () => {
    scrollPanel(document.getElementById('clean'));
    pulse($('#clean'));
  });
}

function pulse(el) {
  if (!el || !el.animate) return;
  el.animate(
    [{ boxShadow: '0 0 0 0 rgba(30,107,91,.35)' }, { boxShadow: '0 0 0 10px rgba(30,107,91,0)' }],
    { duration: 800, easing: 'ease-out' }
  );
}

function scrollPanel(el) {
  if (!el) return;
  if (document.documentElement.scrollHeight <= window.innerHeight + 4) return;
  const y = el.getBoundingClientRect().top + window.scrollY - 16;
  window.scrollTo({ top: Math.max(0, y), behavior: 'smooth' });
}

function renderAgents() {
  const agents = (S.data.agents || []);
  const withData = agents.filter((a) => a.count > 0).sort((a, b) => b.count - a.count);
  const empty = agents.filter((a) => a.count === 0).sort((a, b) => a.name.localeCompare(b.name));
  const shown = withData.concat(empty).slice(0, 6);
  const rest = withData.concat(empty).slice(6);

  if (!withData.length) {
    $('#agents').innerHTML = `
      <div class="empty-state" style="grid-column:1/-1;padding:26px 10px">
        <b>还没有发现会话文件</b>
        已检查 ${empty.length} 个 Agent 的数据目录，点击右上角重新扫描
      </div>`;
  } else {
    $('#agents').innerHTML = shown.map((a) => {
      const muted = a.count === 0;
      const shortName = a.name.replace(/\s+(Code|CLI)$/i, '');
      const meta = a.count
        ? (a.lastSync ? '最近 ' + esc(relTime(new Date(a.lastSync).toISOString())) : '暂无记录')
        : (a.mode === 'detect' ? '已识别 · 待适配' : (a.mode === 'readonly' ? '只读索引' : '已安装 · 暂无会话'));
      return `
      <article class="agent ${S.agent === a.id ? 'is-active' : ''} ${muted ? 'is-empty' : ''}" data-agent="${esc(a.id)}"
        title="${esc(a.name)}${muted ? '（' + esc(meta) + '）' : (a.mode === 'readonly' ? '（会话存储在共享数据库中，占用不计入文件统计）' : '')}"
        ${muted ? 'aria-disabled="true"' : 'tabindex="0" role="button" aria-pressed="' + (S.agent === a.id) + '"'}>
        <div class="agent__head">
          <span class="agent__logo" style="${tintStyle(a.id)}">${agentLogo(a.id)}</span>
          <div style="min-width:0">
            <span class="agent__name">${esc(shortName)}</span>
            <span class="agent__vendor">${esc(a.vendor)}</span>
          </div>
        </div>
        <div class="agent__stats">
          <div class="agent__stat"><b class="num">${fmtNum(a.count)}</b><span>会话</span></div>
          <div class="agent__stat"><b class="num">${fmtBytesParts(a.bytes)[0]}<small style="font-size:9px;font-weight:500">${fmtBytesParts(a.bytes)[1]}</small></b><span>占用</span></div>
        </div>
        <div class="agent__meta">${meta}</div>
      </article>`;
    }).join('');
  }

  const detect = empty.filter((a) => a.mode === 'detect');
  const trulyEmpty = empty.filter((a) => a.mode !== 'detect');
  const detectInstalled = agents.filter((a) => a.mode === 'detect' && a.installed);
  const notes = [];
  // Each agent is mentioned once: the "not shown" list must not repeat the
  // unreadable-format note above it.
  const hidden = rest.filter((a) => !(a.mode === 'detect' && a.installed));
  if (detectInstalled.length) {
    notes.push(`${detectInstalled.map((a) => esc(a.name)).join('、')}：专有/加密格式，已识别目录、暂未接入`);
  }
  if (hidden.length) {
    // Say what the hidden ones are, not just how many: "WorkBuddy 0 会话" is not a gap.
    notes.push(`另有 ${hidden.length} 个 Agent 未展示：${hidden.map((a) => esc(a.name)).join('、')}`);
  }
  if (!notes.length && trulyEmpty.length) {
    notes.push(`灰色卡片表示检测到目录但暂无会话，共 ${trulyEmpty.length} 个`);
  }
  $('#agentsNote').innerHTML = notes.join(' · ');
  $('#agentDetected').textContent = `${agents.filter((a) => a.installed).length} 个已安装`;

  $$('.agent:not(.is-empty)').forEach((el) => {
    const pick = () => {
      S.agent = S.agent === el.dataset.agent ? 'all' : el.dataset.agent;
      document.getElementById('agentFilter').value = S.agent;
      renderAgents();
      renderTable();
    };
    el.addEventListener('click', pick);
    el.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(); } });
  });
}

function renderBuckets() {
  const buckets = S.data.buckets || [];
  $('#buckets').innerHTML = buckets.map((b) => {
    const disabled = b.count === 0;
    return `
      <div class="clean__row ${disabled ? 'is-disabled' : ''}">
        <label class="check">
          <input type="checkbox" class="bucket-check" data-key="${esc(b.key)}" ${S.checkedBuckets.has(b.key) && !disabled ? 'checked' : ''} ${disabled ? 'disabled' : ''}>
          <span class="check__box"><svg><use href="#i-check"/></svg></span>
        </label>
        <span class="clean__label">${esc(b.label)}</span>
        <span class="clean__num">${fmtNum(b.count)} 个</span>
        <span class="clean__size">${fmtBytes(b.bytes)}</span>
      </div>`;
  }).join('');

  const union = bucketUnion();
  $('#cleanLead').textContent = fmtNum(union.ids.length);
  $('#cleanLeadSize').textContent = fmtBytes(union.bytes);
  $('#cleanBtnLabel').textContent = union.ids.length
    ? `删除选中项（${fmtNum(union.ids.length)} 个 · ${fmtBytes(union.bytes)}）`
    : '选择要清理的会话';
  $('#cleanBtn').disabled = union.ids.length === 0;

  const skipped = buckets.filter((b) => b.count && !S.checkedBuckets.has(b.key));
  const hint = $('#cleanTip');
  if (skipped.length) {
    const bytes = skipped.reduce((n, b) => n + b.bytes, 0);
    const count = skipped.reduce((n, b) => n + b.count, 0);
    hint.hidden = false;
    hint.textContent = bytes
      ? `勾选其余分类可再释放 ${fmtBytes(bytes)}`
      : `勾选其余分类可再清理 ${count} 个会话`;
  } else {
    hint.hidden = true;
  }

  $$('.bucket-check').forEach((el) => {
    el.addEventListener('change', () => {
      if (el.checked) S.checkedBuckets.add(el.dataset.key);
      else S.checkedBuckets.delete(el.dataset.key);
      renderBuckets();
      renderKpis();
    });
  });
}

function statusOf(c) {
  if (c.readonly) return { cls: 'readonly', label: '数据库存储' };
  if (c.inArchiveDir) return { cls: 'archived', label: '源已归档' };
  if (c.archived) return { cls: 'archived', label: '已归档' };
  if (Date.now() - Date.parse(c.mtime) < 86400000) return { cls: 'active', label: '活跃' };
  return { cls: 'recent', label: '近期' };
}

function renderTable() {
  const rows = visibleRows();
  const body = $('#tableBody');
  const wrap = $('.table-wrap');
  const scrollTop = wrap ? wrap.scrollTop : 0;

  $('#countAll').textContent = conversations().length;
  $('#countArchived').textContent = conversations().filter((c) => c.archived).length;

  if (!rows.length) {
    body.innerHTML = `<tr><td colspan="9"><div class="empty-state"><b>${S.scanning ? '正在扫描…' : '没有匹配的会话'}</b>${S.scanning ? '正在读取本机 Agent 数据' : '试试调整筛选条件或重新扫描'}</div></td></tr>`;
  } else {
    body.innerHTML = rows.map((c) => {
      const st = statusOf(c);
      const large = c.size > 20 * 1024 * 1024;
      return `
      <tr data-id="${esc(c.id)}" class="${S.activeId === c.id ? 'is-active' : ''} ${S.selected.has(c.id) ? 'is-selected' : ''}" tabindex="0">
        <td><label class="check" data-stop><input type="checkbox" class="row-check" data-id="${esc(c.id)}" ${S.selected.has(c.id) ? 'checked' : ''} aria-label="选择 ${esc(c.title)}"><span class="check__box"><svg><use href="#i-check"/></svg></span></label></td>
        <td><span class="agent-tile" style="${tintStyle(c.agent)}" title="${esc(c.agent)}">${agentLogo(c.agent)}</span></td>
        <td class="c-project"><span class="project-tag" title="${esc(c.projectPath || c.project)}">${esc(c.project)}</span></td>
        <td class="c-title" title="${esc(c.path)}">${esc(c.title)}</td>
        <td class="c-num">${fmtNum(c.messages)}</td>
        <td class="c-size">${c.size ? fmtBytes(c.size) : '—'}${c.compressed ? '<span>zst</span>' : ''}</td>
        <td class="c-time">${esc(relTime(c.mtime))}</td>
        <td><span class="pill pill--${c.readonly ? 'readonly' : (large && !c.archived ? 'large' : st.cls)}">${c.readonly ? st.label : (large && !c.archived ? '大文件' : st.label)}</span></td>
        <td class="c-ops">
          <div class="row-actions" data-stop>
            ${c.inArchiveDir
              ? '<button type="button" disabled title="位于 Agent 自己的归档目录"><svg><use href="#i-archive"/></svg></button>'
              : `<button type="button" data-act="archive" title="${c.archived ? '取消归档' : '归档'}"><svg><use href="#i-archive"/></svg></button>`}
            <button type="button" data-act="reveal" title="打开所在位置"><svg><use href="#i-external"/></svg></button>
            ${c.readonly && !c.path.includes('#')
              ? '<button type="button" disabled title="该来源为只读索引，不支持删除"><svg><use href="#i-trash"/></svg></button>'
              : `<button type="button" data-act="delete" class="danger" title="${c.readonly ? '从 Agent 数据库中删除这条会话' : '删除源文件'}"><svg><use href="#i-trash"/></svg></button>`}
            ${c.readonly && !c.path.includes('#')
              ? '<button type="button" disabled title="该来源为只读索引，不支持删除"><svg><use href="#i-bolt"/></svg></button>'
              : '<button type="button" data-act="force" class="danger" title="强制删除（忽略占用与只读属性）"><svg><use href="#i-bolt"/></svg></button>'}
          </div>
        </td>
      </tr>`;
    }).join('');
  }

  if (wrap) wrap.scrollTop = scrollTop;
  bindTable();
  renderBulk();
}

function bindTable() {
  $$('#tableBody tr[data-id]').forEach((tr) => {
    tr.addEventListener('click', (e) => {
      if (e.target.closest('[data-stop]')) return;
      selectRow(tr.dataset.id);
    });
    tr.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') selectRow(tr.dataset.id);
    });
  });

  $$('.row-check').forEach((cb) => {
    cb.addEventListener('change', () => {
      const id = cb.dataset.id;
      if (cb.checked) S.selected.add(id);
      else S.selected.delete(id);
      cb.closest('tr').classList.toggle('is-selected', cb.checked);
      renderBulk();
      syncCheckAll();
    });
  });

  $$('#tableBody [data-act]').forEach((btn) => {
    btn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const id = btn.closest('tr').dataset.id;
      const row = conversations().find((c) => c.id === id);
      if (!row) return;
      const act = btn.dataset.act;
      if (act === 'archive') await toggleArchive([id], !row.archived);
      if (act === 'reveal') await reveal(id);
      if (act === 'delete') askDelete([id]);
      if (act === 'force') askForceDelete([id]);
    });
  });

  syncCheckAll();
}

function syncCheckAll() {
  const visible = visibleRows();
  const all = $('#checkAll');
  const picked = visible.filter((c) => S.selected.has(c.id));
  all.checked = visible.length > 0 && picked.length === visible.length;
  all.indeterminate = picked.length > 0 && picked.length < visible.length;
}

function renderBulk() {
  const n = S.selected.size;
  $('#bulkBar').hidden = n === 0;
  $('#bulkCount').textContent = `已选 ${n} 项`;
}

function renderStorage() {
  const t = S.data.totals;
  const [v, u] = fmtBytesParts(t.bytes);
  $('#storageSize').textContent = `${v} ${u}`;
  const pct = t.bytes ? Math.min(100, Math.round((bucketUnion().bytes / t.bytes) * 100)) : 0;
  $('#storageBar').style.width = pct + '%';
  $('#storageMeta').textContent = `${fmtNum(t.conversations)} 个会话文件 · 可清理 ${pct}%`;
}

/* ---------------------------------------------------------------- preview */

async function selectRow(id) {
  S.activeId = id;
  renderTable();
  await renderPreview();
}

async function renderPreview() {
  const body = $('#previewBody');
  const folderBtn = $('#openFolderBtn');
  const c = conversations().find((x) => x.id === S.activeId);
  if (!c) {
    folderBtn.hidden = true;
    body.innerHTML = `<div class="preview__empty"><svg viewBox="0 0 24 24" aria-hidden="true"><use href="#i-chat"/></svg><p>选择左侧任意会话，查看对话内容</p></div>`;
    return;
  }

  folderBtn.hidden = false;
  body.innerHTML = `
    <div class="preview__head">
      <span class="agent-tile" style="${tintStyle(c.agent)}">${agentLogo(c.agent)}</span>
      <div style="min-width:0">
        <div class="preview__title">${esc(c.title)}</div>
        <div class="preview__meta">${esc(c.project)} · ${fmtNum(c.messages)} 条消息 · ${esc(relTime(c.mtime))}</div>
      </div>
    </div>
    <div class="preview__msgs" id="previewMsgs"></div>`;

  const msgs = $('#previewMsgs');
  if (S.previews.has(c.id)) return paintPreview(c, S.previews.get(c.id));
  msgs.innerHTML = `<div class="skeleton" style="margin:8px 0"></div><div class="skeleton" style="margin:8px 0"></div><div class="skeleton" style="margin:8px 0"></div>`;

  try {
    const data = BRIDGE
      ? await BRIDGE.preview(c.id)
      : await api(`/api/preview?id=${encodeURIComponent(c.id)}`);
    S.previews.set(c.id, data);
    if (S.activeId === c.id) paintPreview(c, data);
  } catch (err) {
    if (S.activeId === c.id) {
      msgs.innerHTML = `<div class="preview__missing">无法预览这个会话<br><span style="color:var(--faint)">${esc(errText(err))}</span></div>`;
    }
  }
}

function paintPreview(c, data) {
  const msgs = $('#previewMsgs');
  if (!msgs) return;
  const list = (data.messages || []).map((m) => `
    <div class="msg msg--${m.role === 'user' ? 'user' : 'assistant'}">${esc(m.text)}${m.time ? `<time>${esc(fmtClock(m.time))}</time>` : ''}</div>`).join('');
  msgs.innerHTML = list || `<div class="preview__missing">这个文件里没有可读的对话内容</div>`;
  if (data.truncated) {
    const hint = document.createElement('p');
    hint.className = 'preview__hint';
    hint.textContent = data.hint || '仅显示会话开头部分';
    msgs.appendChild(hint);
  }
}

/* ---------------------------------------------------------------- actions */

async function toggleArchive(ids, archived) {
  try {
    for (const id of ids) {
      if (BRIDGE) await BRIDGE.archive(id, archived);
      else await api('/api/archive', { method: 'POST', body: { id, archived } });
    }
    toast(archived ? `已归档 ${ids.length} 个会话` : `已取消归档 ${ids.length} 个会话`);
    await load();
  } catch (err) {
    toast(errText(err), 'error');
  }
}

async function reveal(id) {
  try {
    if (BRIDGE) await BRIDGE.reveal(id);
    else await api('/api/reveal', { method: 'POST', body: { id } });
    toast('已在资源管理器中打开');
  } catch (err) {
    toast(errText(err), 'error');
  }
}

function askDelete(ids) {
  const rows = ids.map((id) => conversations().find((c) => c.id === id)).filter(Boolean);
  if (!rows.length) return;
  const dbRows = rows.filter((r) => r.readonly);
  const fileRows = rows.filter((r) => !r.readonly);
  const bytes = rows.reduce((n, r) => n + r.size, 0);
  const preview = rows.slice(0, 6).map((r) => `<div><b>${esc(r.title)}</b><span>${r.readonly ? '数据库记录' : fmtBytes(r.size)}</span></div>`).join('');
  const more = rows.length > 6 ? `<div><b>… 以及另外 ${rows.length - 6} 个会话</b><span></span></div>` : '';
  const onlyDb = dbRows.length === rows.length;

  const modeBlock = onlyDb ? `
      <div class="warn"><svg><use href="#i-warning"/></svg><span>这些会话存在 Agent 自己的数据库里，会按会话 ID 逐表删除；删除后原应用也不会再显示。</span></div>`
    : `
      <div class="mode">
        <label><input type="radio" name="delmode" value="recycle" ${S.deleteMode === 'recycle' ? 'checked' : ''}>
          <span><b>移入回收站（推荐）</b><span>可以恢复，但空间要清空回收站后才会释放</span></span></label>
        <label><input type="radio" name="delmode" value="purge" ${S.deleteMode === 'purge' ? 'checked' : ''}>
          <span><b>永久删除</b><span>立即释放空间，无法恢复</span></span></label>
      </div>
      <div class="warn"><svg><use href="#i-warning"/></svg><span>操作只影响上面列出的会话，不会修改 Agent 的其它数据${dbRows.length ? `；其中 ${dbRows.length} 条在数据库中，按记录删除` : ''}。</span></div>`;

  openModal({
    title: rows.length === 1 ? '删除会话' : `删除 ${rows.length} 个会话`,
    body: `
      <p>这会移除 ${rows.length} 个会话${fileRows.length ? `，释放 <b>${fmtBytes(fileRows.reduce((n, r) => n + r.size, 0))}</b>` : ''}。删除后对话将无法再从这个工具或原 Agent 中打开。</p>
      <div class="file-list">${preview}${more}</div>
      ${modeBlock}`,
    actions: [
      { label: '取消', cls: 'btn--ghost', close: true },
      { label: '强制删除', cls: 'btn--danger', onClick: () => { closeModal(); askForceDelete(ids); } },
      { label: '删除', cls: 'btn--danger', onClick: async (btn) => {
          const mode = onlyDb ? 'purge' : $('input[name="delmode"]:checked').value;
          S.deleteMode = mode;
          btn.disabled = true;
          btn.textContent = '正在删除…';
          try {
            const res = BRIDGE
              ? await BRIDGE.remove(ids, mode === 'purge')
              : await api('/api/delete', { method: 'POST', body: { ids, purge: mode === 'purge' } });
            closeModal();
            S.selected.clear();
            if (rows.some((r) => r.id === S.activeId)) S.activeId = null;
            S.previews.clear();
            await load();
            if (res.errors && res.errors.length) {
              toast(`已删除 ${res.deleted.length} 个，${res.errors.length} 个失败：${res.errors[0].error}`, 'error');
            } else {
              toast(`已删除 ${res.deleted.length} 个会话${res.freed ? `，释放 ${fmtBytes(res.freed)}` : ''}`);
            }
          } catch (err) {
            btn.disabled = false;
            btn.textContent = '删除';
            toast(errText(err), 'error');
          }
        } },
    ],
  });
}

function askForceDelete(ids) {
  const rows = ids.map((id) => conversations().find((c) => c.id === id)).filter(Boolean);
  if (!rows.length) return;
  const preview = rows.slice(0, 6).map((r) => `<div><b>${esc(r.title)}</b><span>${r.readonly ? '数据库记录' : fmtBytes(r.size)}</span></div>`).join('');
  const more = rows.length > 6 ? `<div><b>… 以及另外 ${rows.length - 6} 个会话</b><span></span></div>` : '';

  openModal({
    title: `强制删除 ${rows.length} 个会话`,
    body: `
      <p>强制删除会绕过回收站：清掉只读属性、重试占用中的文件，并从 Agent 数据库中逐表移除记录，最后 <code>VACUUM</code> 回收空间。</p>
      <div class="file-list">${preview}${more}</div>
      <div class="warn"><svg><use href="#i-warning"/></svg><span>无法恢复，也不会进入回收站。如果 Agent 正在运行，它可能仍持有数据库锁，建议先关闭对应 Agent。</span></div>`,
    actions: [
      { label: '取消', cls: 'btn--ghost', close: true },
      { label: '确认强制删除', cls: 'btn--danger', onClick: async (btn) => {
          btn.disabled = true;
          btn.textContent = '正在强制删除…';
          try {
            const res = BRIDGE
              ? await BRIDGE.forceRemove(ids)
              : await api('/api/force-delete', { method: 'POST', body: { ids } });
            closeModal();
            S.selected.clear();
            if (rows.some((r) => r.id === S.activeId)) S.activeId = null;
            S.previews.clear();
            await load();
            if (res.errors && res.errors.length) {
              toast(`强制删除 ${res.deleted.length} 个，${res.errors.length} 个失败：${res.errors[0].error}`, 'error');
            } else {
              toast(`已强制删除 ${res.deleted.length} 个会话${res.freed ? `，释放 ${fmtBytes(res.freed)}` : ''}`);
            }
          } catch (err) {
            btn.disabled = false;
            btn.textContent = '确认强制删除';
            toast(errText(err), 'error');
          }
        } },
    ],
  });
}

/* ---------------------------------------------------------------- modal */

let modalCloser = null;

function openModal({ title, body, actions }) {
  const modal = $('#modal');
  $('#modalTitle').textContent = title;
  $('#modalBody').innerHTML = body;
  $('#modalFoot').innerHTML = '';
  (actions || []).forEach((a) => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `btn ${a.cls || 'btn--ghost'}`;
    btn.textContent = a.label;
    btn.addEventListener('click', () => {
      if (a.close) return closeModal();
      a.onClick && a.onClick(btn);
    });
    $('#modalFoot').appendChild(btn);
  });
  modal.hidden = false;
  modalCloser = () => closeModal();
  document.addEventListener('keydown', escClose);
  setTimeout(() => $('#modalFoot .btn:last-child')?.focus(), 30);
}

function closeModal() {
  $('#modal').hidden = true;
  modalCloser = null;
  document.removeEventListener('keydown', escClose);
}

function escClose(e) {
  if (e.key === 'Escape') closeModal();
}

/* ---------------------------------------------------------------- events */

function bindChrome() {
  document.addEventListener('click', (e) => {
    if (e.target.matches('[data-close]')) closeModal();
  });

  $('#checkAll').addEventListener('change', (e) => {
    const rows = visibleRows();
    if (e.target.checked) rows.forEach((r) => S.selected.add(r.id));
    else rows.forEach((r) => S.selected.delete(r.id));
    renderTable();
  });

  $('#agentFilter').addEventListener('change', (e) => {
    S.agent = e.target.value;
    renderAgents();
    renderTable();
  });

  $('#statusFilter').addEventListener('change', (e) => {
    S.status = e.target.value;
    renderTable();
  });

  const onSearch = (value) => {
    S.query = value;
    document.getElementById('tableSearch').value = value;
    document.getElementById('globalSearch').value = value;
    renderTable();
  };
  $('#globalSearch').addEventListener('input', (e) => onSearch(e.target.value));
  $('#tableSearch').addEventListener('input', (e) => onSearch(e.target.value));

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      if (VS.current === 'sessions') $('#sessionSearch').focus();
      else { setView('sessions'); $('#sessionSearch').focus(); }
    }
  });

  $$('.tab').forEach((tab) => {
    tab.addEventListener('click', () => {
      $$('.tab').forEach((t) => { t.classList.remove('is-active'); t.setAttribute('aria-selected', 'false'); });
      tab.classList.add('is-active');
      tab.setAttribute('aria-selected', 'true');
      S.tab = tab.dataset.tab;
      renderTable();
    });
  });

  $('#cleanBtn').addEventListener('click', () => {
    const union = bucketUnion();
    if (union.ids.length) askDelete(union.ids);
  });

  $('#cleanHint').addEventListener('click', () => {
    openModal({
      title: '清理规则',
      body: `<p>清理中心只按「内容少、时间久」找出低价值会话，<b>不会因为文件大就选中它</b>——长对话往往是最有价值的内容。</p>
        <p style="margin-top:10px">打开时<b>默认一个都不勾选</b>，必须你亲手勾选才会进入删除清单。</p>
        <p style="margin-top:10px">删除作用于 Agent 的原始文件，默认「移入回收站」，可以恢复；「永久删除」才会立即释放空间且不可恢复。</p>`,
      actions: [{ label: '知道了', cls: 'btn--dark', close: true }],
    });
  });

  $('#rescanBtn').addEventListener('click', async () => {
    try {
      S.scanning = true;
      renderScanState();
      if (BRIDGE) await BRIDGE.rescan();
      else await api('/api/scan', { method: 'POST', body: {} });
      schedulePoll();
      toast('已开始重新扫描');
    } catch (err) {
      S.scanning = false;
      toast(errText(err), 'error');
    }
  });

  $('#bulkArchive').addEventListener('click', () => {
    const ids = Array.from(S.selected);
    if (!ids.length) return;
    toggleArchive(ids, true).then(() => { S.selected.clear(); renderTable(); });
  });
  $('#bulkDelete').addEventListener('click', () => {
    const ids = Array.from(S.selected);
    if (!ids.length) return;
    askDelete(ids);
  });

  $('#openFolderBtn').addEventListener('click', () => {
    if (S.activeId) reveal(S.activeId);
  });

  bindViewEvents();
}

/* ---------------------------------------------------------------- views */

const VIEWS = ['overview', 'sessions', 'projects', 'stats', 'settings'];
const VS = {
  current: 'overview',
  q: '',
  agent: 'all',
  sort: 'recent',
  project: null,
  activeId: null,
  transcript: null,
  limit: 0,
  stars: new Set(),
  starredOnly: false,
  promptsOnly: false,
  view: [],
  focusSeq: 0,
  searchTimer: null,
  loading: false,
};
const MODE_LABEL = { file: '文件级可删', readonly: '数据库可删', mixed: '文件+数据库', detect: '待适配' };
const MODE_HINT = {
  file: '可直接删除源文件',
  readonly: '会话存在 Agent 数据库里，按记录删除',
  detect: '专有或加密格式，暂未接入',
};

function agentName(id) {
  const a = (S.data && S.data.agents || []).find((x) => x.id === id);
  return a ? a.name : id;
}

function setView(name) {
  if (!VIEWS.includes(name)) name = 'overview';
  VS.current = name;
  $$('.nav__item').forEach((n) => n.classList.toggle('is-active', n.dataset.view === name));
  $$('.view').forEach((v) => v.classList.toggle('is-active', v.dataset.view === name));
  renderActiveView();
}

function renderActiveView() {
  if (!S.data) return;
  if (VS.current === 'sessions') { renderAgentOptions(); renderSessionRows(); if (VS.activeId) openTranscript(VS.activeId, true); }
  if (VS.current === 'projects') renderProjects();
  if (VS.current === 'stats') renderStats();
  if (VS.current === 'settings') renderSettings();
}

function sessionListData() {
  const q = VS.q.trim().toLowerCase();
  let rows = conversations().slice();
  if (VS.project) rows = rows.filter((c) => (c.projectPath || c.project) === VS.project);
  if (VS.agent !== 'all') rows = rows.filter((c) => c.agent === VS.agent);
  if (VS.starredOnly) rows = rows.filter((c) => VS.stars.has(c.id));
  if (q) rows = rows.filter((c) => `${c.title} ${c.project} ${c.path} ${agentName(c.agent)}`.toLowerCase().includes(q));
  const sorters = {
    recent: (a, b) => Date.parse(b.mtime) - Date.parse(a.mtime),
    size: (a, b) => b.size - a.size,
    messages: (a, b) => b.messages - a.messages,
  };
  return rows.sort(sorters[VS.sort] || sorters.recent);
}

function renderSessionRows() {
  if (VS.activeId && !conversations().some((c) => c.id === VS.activeId)) {
    VS.activeId = null;
    VS.transcript = null;
    const pane = $('#transcript');
    if (pane) pane.hidden = true;
    const empty = $('#transcriptEmpty');
    if (empty) empty.hidden = false;
  }
  const rows = sessionListData();
  const box = $('#sessionRows');
  if (!box) return;
  const count = $('#listCount');
  if (count) count.textContent = `${rows.length} 个会话`;
  const meta = $('#sessionsMeta');
  if (meta) meta.textContent = VS.project ? `项目：${VS.project}` : '按最近更新排序';

  if (!rows.length) {
    box.innerHTML = `<div class="empty-state"><b>没有匹配的会话</b>换个关键词或筛选条件试试</div>`;
    return;
  }
  const capped = rows.slice(0, 500);
  box.innerHTML = capped.map((c) => {
    const st = statusOf(c);
    return `<button type="button" class="srow${c.id === VS.activeId ? ' is-active' : ''}" data-id="${esc(c.id)}" role="option" aria-selected="${c.id === VS.activeId}">
      <span class="srow__logo" style="${tintStyle(c.agent)}">${agentLogo(c.agent)}</span>
      <span class="srow__main">
        <b class="srow__title">${VS.stars.has(c.id) ? '<i class="star">★</i>' : ''}${esc(c.title || '未命名会话')}</b>
        <span class="srow__sub">${esc(c.project || '未知项目')} · ${relTime(c.mtime)}</span>
      </span>
      <span class="srow__side">
        <span class="pill pill--sm pill--${esc(st.cls)}">${esc(st.label)}</span>
        <span class="srow__num">${fmtNum(c.messages)} 条 · ${fmtBytes(c.size)}</span>
      </span>
    </button>`;
  }).join('') + (rows.length > capped.length ? `<p class="list-more">仅显示前 ${capped.length} 条，用搜索缩小范围</p>` : '');

  $$('.srow', box).forEach((el) => el.addEventListener('click', () => openTranscript(el.dataset.id)));

  const starBtn = $('#starFilter');
  if (starBtn) {
    starBtn.classList.toggle('is-on', VS.starredOnly);
    starBtn.setAttribute('aria-pressed', String(VS.starredOnly));
    starBtn.textContent = VS.stars.size ? `★ 收藏 ${VS.stars.size}` : '★ 收藏';
  }
}

/* ------------------------------------------------------- content search */

async function runContentSearch() {
  const q = VS.q.trim();
  const box = $('#sessionRows');
  if (!box) return;
  if (q.length < 2) {
    renderSessionRows();
    return;
  }
  box.innerHTML = '<p class="list-more">正在搜索对话内容…</p>';
  let res;
  try {
    res = BRIDGE ? await BRIDGE.search(q) : { ready: true, groups: [] };
  } catch (err) {
    box.innerHTML = `<div class="empty-state"><b>搜索失败</b>${esc(errText(err))}</div>`;
    return;
  }
  if (!res.ready) {
    renderSessionRows();
    const note = document.createElement('p');
    note.className = 'list-more';
    note.textContent = '内容索引还在建立，先显示标题匹配结果；稍后再搜可搜到对话正文';
    box.prepend(note);
    return;
  }
  const count = $('#listCount');
  if (count) count.textContent = `${res.groups.length} 个会话命中`;
  if (!res.groups.length) {
    box.innerHTML = `<div class="empty-state"><b>没有找到包含该内容的会话</b>索引只覆盖可读取的会话文件</div>`;
    return;
  }
  box.innerHTML = res.groups.map((g) => {
    const c = g.conversation;
    return `<button type="button" class="srow srow--hit${c.id === VS.activeId ? ' is-active' : ''}" data-id="${esc(c.id)}" data-seq="${g.hits[0].seq}">
      <span class="srow__logo" style="${tintStyle(c.agent)}">${agentLogo(c.agent)}</span>
      <span class="srow__main">
        <b class="srow__title">${esc(c.title || '未命名会话')}</b>
        <span class="srow__sub">${esc(c.project || '未知项目')} · ${relTime(c.mtime)}</span>
        ${g.hits.map((h) => `<span class="hit"><em>${h.role === 'user' ? '你' : esc(agentName(c.agent))}</em>${esc(h.snippet)}</span>`).join('')}
      </span>
    </button>`;
  }).join('');
  $$('.srow', box).forEach((el) => el.addEventListener('click', async () => {
    await openTranscript(el.dataset.id, false, Number(el.dataset.seq) || 0);
  }));
}

async function openTranscript(id, silent = false, focusSeq = 0) {
  const c = conversations().find((x) => x.id === id);
  if (!c) return;
  VS.activeId = id;
  VS.focusSeq = focusSeq;
  VS.loading = true;
  $$('.srow').forEach((el) => el.classList.toggle('is-active', el.dataset.id === id));
  if (!silent) {
    const empty = $('#transcriptEmpty');
    if (empty) empty.hidden = true;
    const pane = $('#transcript');
    if (pane) { pane.hidden = false; $('#tScroll').innerHTML = '<p class="transcript__loading">正在读取对话…</p>'; }
    $('#tTitle').textContent = c.title || '未命名会话';
    $('#tMeta').textContent = `${agentName(c.agent)} · ${c.project || '未知项目'} · ${fmtNum(c.messages)} 条消息 · ${fmtBytes(c.size)}`;
  }
  try {
    const t = BRIDGE
      ? await BRIDGE.transcript(id)
      : await api(`/api/transcript?id=${encodeURIComponent(id)}`);
    VS.transcript = t;
    renderTranscript();
  } catch (err) {
    VS.transcript = null;
    const scroll = $('#tScroll');
    if (scroll) scroll.innerHTML = `<div class="empty-state"><b>无法读取</b>${esc(errText(err))}</div>`;
  } finally {
    VS.loading = false;
  }
}

function mdToHtml(text) {
  const chunks = String(text || '').split('```');
  return chunks.map((chunk, i) => {
    if (i % 2 === 1) {
      const nl = chunk.indexOf('\n');
      const lang = nl > -1 ? chunk.slice(0, nl).trim() : '';
      const code = (nl > -1 ? chunk.slice(nl + 1) : chunk).replace(/\s+$/, '');
      return `<pre class="code"${lang ? ` data-lang="${esc(lang)}"` : ''}><code>${esc(code)}</code></pre>`;
    }
    let html = esc(chunk)
      .replace(/`([^`\n]+)`/g, '<code>$1</code>')
      .replace(/\*\*([^*\n]+)\*\*/g, '<b>$1</b>')
      .replace(/^### (.+)$/gm, '<h4>$1</h4>')
      .replace(/^## (.+)$/gm, '<h3>$1</h3>')
      .replace(/^# (.+)$/gm, '<h3>$1</h3>')
      .replace(/^\s*[-*] (.+)$/gm, '<li>$1</li>');
    html = html.replace(/(?:<li>[^\n]*<\/li>\n?)+/g, (m) => `<ul>${m.replace(/\n/g, '')}</ul>`);
    html = html.replace(/\n{2,}/g, '</p><p>').replace(/\n/g, '<br>');
    return html.trim() ? `<p>${html}</p>` : '';
  }).join('');
}

const PAGE = 120;

function msgHtml(m, who) {
  const role = m.role === 'user' ? 'user' : 'assistant';
  const label = role === 'user' ? '你' : who;
  return `<article class="msg msg--${role}" data-seq="${m.seq}">
    <div class="msg__gutter"><span class="msg__role">${esc(label)}</span>${m.time ? `<time class="msg__time">${esc(fmtClock(m.time))}</time>` : ''}</div>
    <div class="msg__body">${mdToHtml(m.text)}</div>
  </article>`;
}

function renderTranscript() {
  const t = VS.transcript;
  if (!t) return;
  const c = t.conversation;
  $('#transcriptEmpty').hidden = true;
  $('#transcript').hidden = false;
  $('#tTitle').textContent = c.title || '未命名会话';
  const st = statusOf(c);
  $('#tMeta').textContent = `${agentName(c.agent)} · ${c.project || '未知项目'} · ${fmtNum(c.messages)} 条消息 · ${fmtBytes(c.size)} · ${st.label}`;
  $('#tArchive').textContent = c.archived ? '取消归档' : '归档';
  $('#tArchive').disabled = Boolean(c.inArchiveDir);
  $('#tDelete').disabled = false;
  $('#tDelete').title = c.readonly ? '从 Agent 数据库中删除这条会话' : '删除源文件';
  $('#tExport').disabled = !t.messages.length;
  $('#tCopyPath').disabled = Boolean(c.readonly);
  const starBtn = $('#tStar');
  if (starBtn) {
    const on = VS.stars.has(c.id);
    starBtn.textContent = on ? '★ 已收藏' : '☆ 收藏';
    starBtn.classList.toggle('is-on', on);
  }
  const promptsBtn = $('#tPrompts');
  if (promptsBtn) {
    promptsBtn.textContent = VS.promptsOnly ? '显示全部' : '只看提问';
    promptsBtn.classList.toggle('is-on', VS.promptsOnly);
  }

  const scroll = $('#tScroll');
  VS.view = VS.promptsOnly ? t.messages.filter((m) => m.role === 'user') : t.messages;
  if (!VS.view.length) {
    scroll.innerHTML = `<div class="empty-state"><b>${t.messages.length ? '没有你的提问' : '没有可读的对话内容'}</b>${
      t.messages.length ? '这个会话只有 Agent 的回复' : `${esc(agentName(c.agent))} 将会话保存在数据库里，本工具只索引元数据。`
    }</div>`;
    return;
  }
  VS.limit = PAGE;
  const first = Math.max(0, VS.view.findIndex((m) => m.seq >= VS.focusSeq));
  const head = VS.focusSeq ? VS.view.slice(first, first + PAGE) : VS.view.slice(0, PAGE);
  VS.limit = first + head.length;
  scroll.innerHTML = head.map((m) => msgHtml(m, agentName(c.agent))).join('');
  appendMore(scroll);
  const target = scroll.querySelector(`[data-seq="${VS.focusSeq}"]`);
  if (target) {
    target.classList.add('is-hit');
    target.scrollIntoView({ block: 'center' });
  } else {
    scroll.scrollTop = 0;
  }
  VS.focusSeq = 0;
}

function appendMore(scroll) {
  const t = VS.transcript;
  if (!t || VS.limit >= VS.view.length) return;
  scroll.insertAdjacentHTML('beforeend', `<div class="more-wrap"><button class="ghost-btn" type="button" id="loadMore">显示更多（已显示 ${fmtNum(VS.limit)} / ${fmtNum(VS.view.length)}）</button></div>`);
  $('#loadMore').addEventListener('click', loadMoreMessages);
}

function loadMoreMessages() {
  const t = VS.transcript;
  if (!t) return;
  const scroll = $('#tScroll');
  const wrap = $('#loadMore')?.parentElement;
  const start = VS.limit;
  const next = VS.view.slice(start, start + PAGE);
  VS.limit = start + next.length;
  if (wrap) wrap.remove();
  scroll.insertAdjacentHTML('beforeend', next.map((m) => msgHtml(m, agentName(t.conversation.agent))).join(''));
  appendMore(scroll);
}

function transcriptMarkdown() {
  const t = VS.transcript;
  if (!t) return '';
  const c = t.conversation;
  const head = `# ${c.title || '未命名会话'}\n\n- Agent：${agentName(c.agent)}\n- 项目：${c.project || '未知'}\n- 文件：\`${c.path}\`\n- 消息数：${t.total}\n- 最后更新：${c.mtime}\n\n---\n\n`;
  const body = t.messages.map((m) => `### ${m.role === 'user' ? '用户' : agentName(c.agent)}${m.time ? ` · ${m.time}` : ''}\n\n${m.text}`).join('\n\n');
  return head + body;
}

function downloadText(name, text) {
  const blob = new Blob([text], { type: 'text/markdown;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

async function copyText(text, okMsg) {
  try {
    await navigator.clipboard.writeText(text);
    toast(okMsg || '已复制');
    return;
  } catch { /* fall through to the legacy path */ }
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
    toast(okMsg || '已复制');
  } catch {
    toast('复制失败，请手动选择文本', 'error');
  }
  ta.remove();
}

function renderProjects() {
  const box = $('#projectCards');
  if (!box) return;
  const map = new Map();
  for (const c of conversations()) {
    const key = c.projectPath || c.project || '未知项目';
    const name = c.project || key.split(/[\\/]/).filter(Boolean).pop() || '未知项目';
    if (!map.has(key)) map.set(key, { key, name, path: c.projectPath || '', count: 0, bytes: 0, messages: 0, last: 0, agents: new Set() });
    const p = map.get(key);
    p.count += 1;
    p.bytes += c.size;
    p.messages += c.messages;
    p.last = Math.max(p.last, Date.parse(c.mtime) || 0);
    p.agents.add(c.agent);
  }
  const projects = Array.from(map.values()).sort((a, b) => b.bytes - a.bytes || b.last - a.last);
  const meta = $('#projectsMeta');
  if (meta) meta.textContent = `${projects.length} 个项目 · 按占用排序`;
  if (!projects.length) {
    box.innerHTML = `<div class="empty-state"><b>还没有项目</b>扫描到会话后会自动按项目归类</div>`;
    return;
  }
  box.innerHTML = projects.map((p) => `
    <button type="button" class="project-card" data-project="${esc(p.key)}">
      <div class="project-card__head">
        <span class="project-card__icon">${agentLogo(Array.from(p.agents)[0])}</span>
        <b>${esc(p.name)}</b>
      </div>
      <p class="project-card__path" title="${esc(p.path)}">${esc(p.path || '未记录路径')}</p>
      <div class="project-card__stats">
        <span><b>${fmtNum(p.count)}</b>会话</span>
        <span><b>${fmtNum(p.messages)}</b>消息</span>
        <span><b>${fmtBytes(p.bytes)}</b>占用</span>
      </div>
      <div class="project-card__foot">
        <span>${Array.from(p.agents).map((a) => esc(agentName(a))).join(' · ')}</span>
        <span>${p.last ? relTime(new Date(p.last).toISOString()) : '—'}</span>
      </div>
    </button>`).join('');
  $$('.project-card', box).forEach((el) => el.addEventListener('click', () => {
    VS.project = el.dataset.project;
    VS.agent = 'all';
    VS.q = '';
    const s = $('#sessionSearch');
    if (s) s.value = '';
    setView('sessions');
  }));
}

function renderStats() {
  const box = $('#statsBody');
  if (!box) return;
  const rows = conversations();
  const totalBytes = rows.reduce((n, c) => n + c.size, 0) || 1;
  const byAgent = new Map();
  for (const c of rows) {
    const a = byAgent.get(c.agent) || { id: c.agent, count: 0, bytes: 0, messages: 0 };
    a.count += 1; a.bytes += c.size; a.messages += c.messages;
    byAgent.set(c.agent, a);
  }
  const agents = Array.from(byAgent.values()).sort((a, b) => b.bytes - a.bytes);

  const months = new Map();
  const now = new Date();
  for (let i = 5; i >= 0; i -= 1) {
    const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
    months.set(`${d.getFullYear()}-${d.getMonth() + 1}`, { label: `${d.getMonth() + 1}月`, count: 0 });
  }
  for (const c of rows) {
    const t = Date.parse(c.mtime);
    if (!t) continue;
    const d = new Date(t);
    const key = `${d.getFullYear()}-${d.getMonth() + 1}`;
    if (months.has(key)) months.get(key).count += 1;
  }
  const monthList = Array.from(months.values());
  const maxMonth = Math.max(1, ...monthList.map((m) => m.count));

  const projects = new Map();
  for (const c of rows) {
    const key = c.project || '未知项目';
    projects.set(key, (projects.get(key) || 0) + c.size);
  }
  const topProjects = Array.from(projects.entries()).sort((a, b) => b[1] - a[1]).slice(0, 8);
  const maxProject = Math.max(1, ...topProjects.map(([, v]) => v));

  const buckets = (S.data && S.data.buckets) || [];
  const cleanBytes = buckets.reduce((n, b) => n + b.bytes, 0);
  const readonly = rows.filter((c) => c.readonly).length;

  box.innerHTML = `
    <section class="stat-card">
      <h2>按 Agent 分布</h2>
      <p class="stat-card__sub">占用 ${fmtBytes(totalBytes)} · 会话 ${fmtNum(rows.length)}</p>
      <div class="bars">
        ${agents.map((a) => `<div class="bar-row">
          <span class="bar-row__label">${esc(agentName(a.id))}</span>
          <span class="bar"><i style="width:${Math.max(2, (a.bytes / totalBytes) * 100)}%;background:${AGENT_TINT[a.id] || '#141210'}"></i></span>
          <span class="bar-row__value">${fmtBytes(a.bytes)}<em>${fmtNum(a.count)} 会话</em></span>
        </div>`).join('') || '<p class="stat-empty">暂无数据</p>'}
      </div>
    </section>

    <section class="stat-card">
      <h2>最近 6 个月</h2>
      <p class="stat-card__sub">按最后更新时间统计的会话数</p>
      <div class="monthbars">
        ${monthList.map((m) => `<div class="monthbar">
          <b>${m.count}</b>
          <span class="monthbar__col"><i style="height:${Math.max(4, (m.count / maxMonth) * 100)}%"></i></span>
          <em>${m.label}</em>
        </div>`).join('')}
      </div>
    </section>

    <section class="stat-card">
      <h2>占用最多的项目</h2>
      <p class="stat-card__sub">共 ${fmtNum(projects.size)} 个项目</p>
      <div class="bars">
        ${topProjects.map(([name, bytes]) => `<div class="bar-row">
          <span class="bar-row__label" title="${esc(name)}">${esc(name)}</span>
          <span class="bar"><i style="width:${Math.max(2, (bytes / maxProject) * 100)}%"></i></span>
          <span class="bar-row__value">${fmtBytes(bytes)}</span>
        </div>`).join('') || '<p class="stat-empty">暂无数据</p>'}
      </div>
    </section>

    <section class="stat-card">
      <h2>清理空间</h2>
      <p class="stat-card__sub">按四类规则估算，只统计可删除的文件</p>
      <div class="stat-kpis">
        <div><b>${fmtBytes(cleanBytes)}</b><span>可清理</span></div>
        <div><b>${fmtNum(readonly)}</b><span>只读会话</span></div>
        <div><b>${fmtNum(buckets.reduce((n, b) => n + b.ids.length, 0))}</b><span>命中会话</span></div>
      </div>
      <div class="bars">
        ${buckets.map((b) => `<div class="bar-row">
          <span class="bar-row__label">${esc(b.label)}</span>
          <span class="bar"><i style="width:${Math.max(2, b.bytes / Math.max(1, cleanBytes) * 100)}%;background:${b.key === 'large' ? '#B5482F' : '#0F7A56'}"></i></span>
          <span class="bar-row__value">${fmtBytes(b.bytes)}<em>${b.ids.length} 个</em></span>
        </div>`).join('') || '<p class="stat-empty">暂无数据</p>'}
      </div>
    </section>`;
}

function renderSettings() {
  const box = $('#settingsBody');
  if (!box) return;
  const agents = (S.data && S.data.agents) || [];
  box.innerHTML = `
    <section class="set-card">
      <h2>数据来源</h2>
      <p class="set-card__sub">已识别的本机 Agent，只读不写，除非你主动删除</p>
      <div class="src-table">
        ${agents.map((a) => `<div class="src-row">
          <span class="src-row__logo" style="${tintStyle(a.id)}">${agentLogo(a.id)}</span>
          <b>${esc(a.name)}</b>
          <span class="src-mode src-mode--${esc(a.mode)}">${esc(MODE_LABEL[a.mode] || a.mode)}</span>
          <em title="${esc((a.roots || []).join('\n'))}">${esc((a.roots || [])[0] || '未发现数据目录')}</em>
          <span class="src-row__num">${fmtNum(a.count)} 个 · ${fmtBytes(a.bytes)}</span>
        </div>`).join('')}
      </div>
      <p class="set-note">${esc(MODE_HINT.readonly)}；${esc(MODE_HINT.detect)}。</p>
    </section>

    <section class="set-card">
      <h2>删除方式</h2>
      <p class="set-card__sub">删除只作用于 Agent 的原始会话文件</p>
      <div class="mode">
        <label><input type="radio" name="setmode" value="recycle" ${S.deleteMode === 'recycle' ? 'checked' : ''}>
          <span><b>移入回收站（推荐）</b><span>可以恢复，清空回收站后才释放空间</span></span></label>
        <label><input type="radio" name="setmode" value="purge" ${S.deleteMode === 'purge' ? 'checked' : ''}>
          <span><b>永久删除</b><span>立即释放空间，无法恢复</span></span></label>
      </div>
    </section>

    <section class="set-card">
      <h2>本机数据</h2>
      <p class="set-card__sub">所有数据都留在本机，不联网、不上传</p>
      <div class="set-actions">
        <button class="btn btn--ghost" type="button" id="setRevealData">打开数据目录</button>
        <button class="btn btn--ghost" type="button" id="setRescan">重新扫描全部 Agent</button>
      </div>
      <p class="set-note">索引状态：${S.data && S.data.scannedAt ? esc(S.data.scannedAt) : '—'}${S.data && S.data.durationMs ? ` · 用时 ${(S.data.durationMs / 1000).toFixed(1)}s` : ''}</p>
    </section>

    <section class="set-card">
      <h2>关于</h2>
      <p class="set-card__sub">AgentHub 0.1.0 · Tauri + Rust · 本机离线</p>
      <p class="set-note">设计参考了这些开源项目：
        <a href="https://github.com/jhlee0409/claude-code-history-viewer" target="_blank" rel="noreferrer">claude-code-history-viewer</a>（多 Agent 统一浏览、分析面板）、
        <a href="https://github.com/HizTam/codex-history-viewer" target="_blank" rel="noreferrer">codex-history-viewer</a>（标签与导出思路）。
      </p>
    </section>`;

  $$('input[name="setmode"]', box).forEach((el) => el.addEventListener('change', () => {
    S.deleteMode = el.value;
    // Deliberately not persisted: an old "permanent" choice must not silently
    // apply to a delete the user triggers days later.
    toast(el.value === 'purge' ? '本次删除方式：永久删除（仅本次生效）' : '删除方式：移入回收站');
  }));
  const rd = $('#setRevealData');
  if (rd) rd.addEventListener('click', () => (BRIDGE ? BRIDGE.revealData() : api('/api/reveal-data', { method: 'POST', body: {} })).catch(() => toast('暂不支持', 'error')));
  const rs = $('#setRescan');
  if (rs) rs.addEventListener('click', () => { setView('overview'); $('#rescanBtn').click(); });
}

function bindViewEvents() {
  $$('.nav__item[data-view]').forEach((btn) => btn.addEventListener('click', () => setView(btn.dataset.view)));

  const filter = $('#sessionAgentFilter');
  if (filter) filter.addEventListener('change', () => { VS.agent = filter.value; renderSessionRows(); });
  const sort = $('#sortSelect');
  if (sort) sort.addEventListener('change', () => { VS.sort = sort.value; renderSessionRows(); });
  const search = $('#sessionSearch');
  if (search) search.addEventListener('input', () => {
    VS.q = search.value;
    clearTimeout(VS.searchTimer);
    VS.searchTimer = setTimeout(() => {
      if (VS.q.trim().length >= 2 && BRIDGE) runContentSearch();
      else renderSessionRows();
    }, 260);
  });

  const starFilter = $('#starFilter');
  if (starFilter) starFilter.addEventListener('click', () => { VS.starredOnly = !VS.starredOnly; renderSessionRows(); });

  const tStar = $('#tStar');
  if (tStar) tStar.addEventListener('click', async () => {
    const c = VS.transcript && VS.transcript.conversation;
    if (!c) return;
    const next = !VS.stars.has(c.id);
    if (next) VS.stars.add(c.id); else VS.stars.delete(c.id);
    renderTranscript();
    renderSessionRows();
    try { if (BRIDGE) await BRIDGE.star(c.id, next); } catch (err) { toast(errText(err), 'error'); }
    toast(next ? '已加入收藏' : '已取消收藏');
  });

  const tPrompts = $('#tPrompts');
  if (tPrompts) tPrompts.addEventListener('click', () => { VS.promptsOnly = !VS.promptsOnly; renderTranscript(); });

  const chip = $('#projectChip');
  if (chip) chip.addEventListener('click', () => { VS.project = null; renderSessionRows(); });

  $('#tCopyPath').addEventListener('click', () => {
    const c = VS.transcript && VS.transcript.conversation;
    if (c) copyText(c.path, '已复制文件路径');
  });
  $('#tExport').addEventListener('click', async () => {
    const c = VS.transcript && VS.transcript.conversation;
    if (!c) return;
    const safe = (c.title || 'session').replace(/[\\/:*?"<>|\s]+/g, '-').slice(0, 60);
    const node = `${safe}-${c.id.slice(0, 6)}`;
    const md = transcriptMarkdown();
    if (BRIDGE) {
      try {
        const path = await BRIDGE.exportMd(node, md);
        toast(`已导出到 ${path}`);
        return;
      } catch (err) {
        toast(errText(err), 'error');
      }
    }
    downloadText(`${node}.md`, md);
  });
  $('#tReveal').addEventListener('click', () => { if (VS.activeId) reveal(VS.activeId); });
  $('#tArchive').addEventListener('click', async () => {
    const c = VS.transcript && VS.transcript.conversation;
    if (!c) return;
    await toggleArchive([c.id], !c.archived);
    await load();
    renderActiveView();
  });
  $('#tDelete').addEventListener('click', () => { if (VS.activeId) askDelete([VS.activeId]); });

  document.addEventListener('keydown', (e) => {
    if (e.target.matches('input, textarea, select')) return;
    if (e.key === '1') setView('overview');
    if (e.key === '2') setView('sessions');
    if (e.key === '3') setView('projects');
    if (e.key === '4') setView('stats');
    if (e.key === '5') setView('settings');
  });
}

/* ---------------------------------------------------------------- shell */

function setupTitlebar() {
  if (!BRIDGE) return;
  document.body.classList.add('tauri');
  const bar = $('#titlebar');
  if (bar) bar.hidden = false;
  const act = (id, cmd) => {
    const el = document.getElementById(id);
    if (el) el.addEventListener('click', () => TAURI.core.invoke(cmd).catch(() => {}));
  };
  act('winMin', 'window_minimize');
  act('winMax', 'window_toggle_maximize');
  act('winClose', 'window_close');
}

/* ---------------------------------------------------------------- boot */

(async function boot() {
  setupTitlebar();
  try {
    await load();
  } catch (err) {
    $('#scanMeta').textContent = BRIDGE ? '读取本机索引失败' : '无法连接本地服务，请确认启动窗口仍在运行';
    $('#tableBody').innerHTML = `<tr><td colspan="9"><div class="empty-state"><b>连接失败</b>${esc(errText(err))}</div></td></tr>`;
  }
  bindChrome();
  renderAll();
})();
