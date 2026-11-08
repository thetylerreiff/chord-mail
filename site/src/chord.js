// The page: three colour blocks, then the keymap plate. One listener drives both: a key acts on the
// inbox (while it's on screen), shows in the orange readout, and presses its cap on the plate.
import { mountInbox, wireCopy } from './inbox.js';
import { ACTS, mountKeymap, plateKey } from './keymap.js';

const $ = (id) => document.getElementById(id);
// A tap on one of the inbox's own buttons always counts: a button you can tap is on screen.
const inbox = mountInbox($('inbox'), { listen: false, toggle: false, onKey: (key) => run(key, true) });
// The plate's resting line, worded for the device in hand.
if (matchMedia('(hover: none) and (pointer: coarse)').matches) $('readout').textContent = 'tap any black key.';
const press = mountKeymap($('board'), $('readout'));

// The orange readout: the key, then what it did.
const glyph = $('glyph');
const verb = $('verb');
function show(act, said) {
  const a = ACTS[act];
  $('key-label').textContent = 'Last key';
  const label = act === 'ctrl+k' ? '^K' : act;
  glyph.textContent = label;
  glyph.classList.toggle('word', label.length > 2);
  glyph.classList.toggle('desc', /^[jgpq,]$/.test(label));
  verb.textContent = said ?? a.verb;
  const box = glyph.parentElement;
  box.classList.remove('hit');
  void box.offsetWidth;
  box.classList.add('hit');
}

// The inbox only takes keys while you can see it.
let inboxInView = true;
new IntersectionObserver(([entry]) => (inboxInView = entry.isIntersecting), { threshold: 0.4 }).observe($('inbox'));

const INBOX_ACTS = new Set(['j', 'k', 'e', 'h', '#', 's', 'z']);
function run(act, tapped = false) {
  let said;
  if (INBOX_ACTS.has(act) && (inboxInView || tapped)) {
    if (act === 'z' && !inbox.canUndo()) said = 'Nothing to undo';
    else if (act !== 'z' && !inbox.count()) said = 'Nothing here';
    else inbox.act(act);
  }
  press(act);
  show(act, said);
}

let on = true;
addEventListener('keydown', (ev) => {
  if (!on || ev.metaKey || ev.altKey || ev.repeat) return;
  if (ev.target.closest('input, textarea, select, [contenteditable]')) return;
  const act = plateKey(ev);
  if (!act) return;
  document.documentElement.classList.add('keyed');
  // Enter and Esc belong to whatever control has focus.
  if ((act === 'enter' || act === 'esc') && ev.target.closest('a, button, summary')) return;
  if (act === '/' || act === '?' || act === 'ctrl+k') ev.preventDefault();
  run(act);
});

// One switch for single-key shortcuts (WCAG 2.1.4), shown in two places and kept in sync.
// Its name stays "Keyboard shortcuts"; aria-pressed carries the state.
const toggles = document.querySelectorAll('.keys-toggle');
toggles.forEach((t) =>
  t.addEventListener('click', () => {
    on = !on;
    toggles.forEach((x) => x.setAttribute('aria-pressed', String(on)));
  }),
);

// The black panel's big number: what's left in the split you're looking at.
const SPLIT_NAME = { primary: 'Primary', updates: 'Updates', promotions: 'Promotions' };
$('inbox').addEventListener('chord:count', (ev) => {
  const { split, left } = ev.detail;
  $('left').textContent = left;
  $('left-label').textContent = `${left === 1 ? 'thread' : 'threads'} left in ${SPLIT_NAME[split]}`;
});

// The plate's readout speaks to screen readers only while the plate is on screen.
new IntersectionObserver(([entry]) => $('readout').setAttribute('aria-live', entry.isIntersecting ? 'polite' : 'off'), { threshold: 0.3 }).observe($('keys'));

// The "every key" link glides down to the plate, unless motion is reduced.
document.querySelector('.down').addEventListener('click', (ev) => {
  ev.preventDefault();
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  $('keys').scrollIntoView({ behavior: still ? 'auto' : 'smooth' });
});

wireCopy($('copy'), $('cmd'));
wireCopy($('copy2'), $('cmd2'));

// Phones: the plate pans; start it on the home row's busy middle (e to j).
const wrap = $('board-wrap');
if (wrap.scrollWidth > wrap.clientWidth) wrap.scrollLeft = (wrap.scrollWidth - wrap.clientWidth) * 0.3;
