// A2: the keymap plate. A hairline drawing of a 60% keyboard; every key Chord binds is filled
// black, e is the one orange key. Press a key and its cap goes down and the readout says what
// it does in Chord. Bindings come from the README's key tables.

const U = 64; // one key unit, in SVG units
const GAP = 6;
const ROWS = [
  [['esc', 1], ['1', 1], ['2', 1], ['3', 1], ['4', 1], ['5', 1], ['6', 1], ['7', 1], ['8', 1], ['9', 1], ['0', 1], ['-', 1], ['=', 1], ['bksp', 2]],
  [['tab', 1.5], ['q', 1], ['w', 1], ['e', 1], ['r', 1], ['t', 1], ['y', 1], ['u', 1], ['i', 1], ['o', 1], ['p', 1], ['[', 1], [']', 1], ['\\', 1.5]],
  [['caps', 1.75], ['a', 1], ['s', 1], ['d', 1], ['f', 1], ['g', 1], ['h', 1], ['j', 1], ['k', 1], ['l', 1], [';', 1], ["'", 1], ['enter', 2.25]],
  [['shift', 2.25], ['z', 1], ['x', 1], ['c', 1], ['v', 1], ['b', 1], ['n', 1], ['m', 1], [',', 1], ['.', 1], ['/', 1], ['shift ', 2.75]],
  [['ctrl', 1.25], ['super', 1.25], ['alt', 1.25], ['space', 6.25], ['alt ', 1.25], ['super ', 1.25], ['menu', 1.25], ['ctrl ', 1.25]],
];

// Every action, from the README's key tables: the cap it presses, a short verb for the big
// readout, and what it does. Shifted actions are their own entries (E is Shift+E).
export const ACTS = {
  j: { cap: 'j', verb: 'Next', does: 'next conversation' },
  k: { cap: 'k', verb: 'Previous', does: 'previous conversation' },
  enter: { cap: 'enter', verb: 'Open', does: 'opens the conversation' },
  esc: { cap: 'esc', verb: 'Back', does: 'back to the list' },
  n: { cap: 'n', verb: 'Next msg', does: 'next message in the thread' },
  p: { cap: 'p', verb: 'Prev msg', does: 'previous message in the thread' },
  tab: { cap: 'tab', verb: 'Next split', does: 'next split: primary, updates, promotions' },
  e: { cap: 'e', verb: 'Done', does: 'done. archived at once' },
  E: { cap: 'e', label: 'shift+e', verb: 'To inbox', does: 'moves it back to the inbox' },
  h: { cap: 'h', verb: 'Snoozed', does: 'snooze until any time you type' },
  s: { cap: 's', verb: 'Starred', does: 'stars it' },
  S: { cap: 's', label: 'shift+s', verb: 'Saved', does: 'saves the attachments to downloads' },
  '#': { cap: '3', verb: 'Trash', does: 'moves it to the trash' },
  '!': { cap: '1', verb: 'Spam', does: 'marks it as spam' },
  l: { cap: 'l', verb: 'Label', does: 'adds a label' },
  v: { cap: 'v', verb: 'Move', does: 'moves it to a label' },
  U: { cap: 'u', label: 'shift+u', verb: 'Unread', does: 'marks it unread' },
  u: { cap: 'u', verb: 'With shift', does: 'with shift: marks it unread' },
  x: { cap: 'x', verb: 'Select', does: 'selects it' },
  r: { cap: 'r', verb: 'Reply', does: 'reply, in neovim' },
  a: { cap: 'a', verb: 'Reply all', does: 'reply all' },
  f: { cap: 'f', verb: 'Forward', does: 'forward' },
  c: { cap: 'c', verb: 'Compose', does: 'compose, in neovim' },
  '/': { cap: '/', verb: 'Search', does: 'search, with gmail syntax' },
  '?': { cap: '/', verb: 'Every key', does: 'shows every key' },
  g: { cap: 'g', verb: 'Go to', does: 'go to: then i, s, t, d, h, a or l' },
  i: { cap: 'i', verb: 'Images', does: 'loads remote images, once' },
  I: { cap: 'i', label: 'shift+i', verb: 'Always', does: 'always loads images from this sender' },
  '.': { cap: '.', verb: 'Quoted', does: 'shows the quoted text' },
  D: { cap: 'd', label: 'shift+d', verb: 'Colours', does: 'shows the original colours' },
  d: { cap: 'd', verb: 'With shift', does: 'with shift: shows the original colours' },
  z: { cap: 'z', verb: 'Undone', does: 'undo. anything, even in gmail' },
  'ctrl+k': { cap: 'ctrl', label: 'ctrl+k', verb: 'Commands', does: 'opens every command' },
  q: { cap: 'q', verb: 'Quit', does: 'quit' },
  shift: { cap: 'shift', verb: 'Shift', does: 'with e, u, s, i or d: the capital bindings' },
};
// The caps that do something are drawn black. A click on a cap runs its first action.
const KEYSET = new Set(Object.values(ACTS).map((a) => a.cap));
const CAP_ACT = {};
for (const [act, a] of Object.entries(ACTS)) CAP_ACT[a.cap] ??= act;
// Shifted legends drawn above the base legend, as on a real keycap.
const SHIFTED = { '1': '!', '3': '#', '/': '?' };

// Labels on leader lines: cap, text, side (up/down), tier (0 near, 1 far), and the line's x in
// key units. Each x runs through a gap between keys in the rows it crosses (or over the blank
// spacebar), so no line strikes a legend.
const CALLOUTS = [
  ['esc', 'esc  back', 'up', 0, 0.84],
  ['3', '#  trash', 'up', 1, 3.72],
  ['e', 'e  done', 'up', 0, 4.0],
  ['ctrl', 'ctrl+k  commands', 'down', 1, 0.75],
  ['z', 'z  undo', 'down', 0, 2.5],
  ['c', 'c  compose', 'down', 0, 4.75],
  ['h', 'h  snooze', 'down', 1, 7.25],
  ['j', 'j k  next / prev', 'down', 0, 8.25],
  ['/', '/  search', 'down', 1, 11.25, 11.72],
];

const NS = 'http://www.w3.org/2000/svg';
const el = (name, attrs = {}, text) => {
  const n = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  if (text != null) n.textContent = text;
  return n;
};

export function drawKeyboard(svg) {
  const width = 15 * U;
  const height = 5 * U;
  const BAND = 92; // room for two tiers of labels above and below
  svg.setAttribute('viewBox', `-8 ${-BAND} ${width + 16} ${height + BAND * 2}`);
  const keys = {};
  const g = el('g', { class: 'board' });
  // The case: one hairline plate around the keys.
  g.append(el('rect', { class: 'case', x: -6, y: -6, width: width + 12, height: height + 12 }));
  ROWS.forEach((row, r) => {
    let x = 0;
    for (const [name, w] of row) {
      const id = name.trim();
      const cap = el('g', { class: `cap${KEYSET.has(id) ? ' bound' : ''}${id === 'e' ? ' hot' : ''}`, 'data-key': id });
      cap.append(el('rect', { x: x + GAP / 2, y: r * U + GAP / 2, width: w * U - GAP, height: U - GAP }));
      const legend = id.length > 1 ? id : id.toUpperCase();
      if (SHIFTED[id]) {
        cap.append(el('text', { x: x + 10, y: r * U + 22 }, SHIFTED[id]));
        cap.append(el('text', { x: x + 10, y: r * U + 46 }, legend));
      } else {
        cap.append(el('text', { x: x + 10, y: r * U + 22, class: id.length > 1 ? 'mod' : '' }, legend));
      }
      g.append(cap);
      keys[id] ??= { cx: x + (w * U) / 2, y: r * U, el: cap };
      x += w * U;
    }
  });
  // Leader lines and labels.
  const notes = el('g', { class: 'notes' });
  const dots = el('g', { class: 'notes dots' });
  for (const [id, text, side, tier, ux, dotUx = ux] of CALLOUTS) {
    const k = keys[id];
    const x = ux * U;
    const dx = dotUx * U;
    // The dot sits on the cap, clear of its legend (legends are top-left). When the dot can't sit
    // on the line's own x, the line jogs sideways along the gap between rows to reach it.
    const y0 = side === 'up' ? k.y + 14 : k.y + U - 14;
    const y1 = side === 'up' ? -22 - tier * 40 : height + 22 + tier * 40;
    const yGap = side === 'up' ? k.y : k.y + U;
    const pts = dx === x ? [[x, y0], [x, y1]] : [[dx, y0], [dx, yGap], [x, yGap], [x, y1]];
    notes.append(el('polyline', { points: pts.map((p) => p.join(',')).join(' '), fill: 'none' }));
    dots.append(el('circle', { cx: dx, cy: y0, r: 4 }));
    notes.append(el('text', { x: x + 6, y: y1 + 4, class: id === 'e' ? 'hot' : '' }, text));
  }
  svg.append(g, notes, dots);
  return keys;
}

// Draws the plate and returns press(act), which presses the act's cap and says what it does.
export function mountKeymap(svg, readout) {
  const keys = drawKeyboard(svg);
  const DEFAULT = readout.textContent;
  let timer;
  function press(act) {
    const a = ACTS[act];
    const k = a && keys[a.cap];
    if (!k) return;
    k.el.classList.remove('down');
    void k.el.getBBox();
    k.el.classList.add('down');
    setTimeout(() => k.el.classList.remove('down'), 200);
    readout.innerHTML = `<b>${a.label ?? act}</b> ${a.does}.`;
    clearTimeout(timer);
    timer = setTimeout(() => (readout.textContent = DEFAULT), 3600);
  }
  svg.addEventListener('click', (ev) => {
    const cap = ev.target.closest('.cap');
    if (cap && CAP_ACT[cap.dataset.key]) press(CAP_ACT[cap.dataset.key]);
  });
  return press;
}

// Maps a keydown to an action, or null. Tab is never taken (it moves focus), Shift and Ctrl on
// their own do nothing, and Ctrl only counts with K.
export function plateKey(ev) {
  if (ev.key === 'Tab') return null;
  if (ev.ctrlKey) return ev.key.toLowerCase() === 'k' ? 'ctrl+k' : null;
  if (ev.key === 'Escape') return 'esc';
  if (ev.key === 'Enter') return 'enter';
  return ev.key.length === 1 && ACTS[ev.key] ? ev.key : null;
}
