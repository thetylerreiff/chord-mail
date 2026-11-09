// A, then the keymap plate. One listener drives both: a key acts on the inbox (while it's on
// screen) and presses its cap on the plate, which says what it does.
import { mountInbox, wireCopy } from './inbox.js';
import { mountKeymap, plateKey } from './keymap.js';

const $ = (id) => document.getElementById(id);
// A tap on one of the inbox's own buttons always counts: a button you can tap is on screen.
const inbox = mountInbox($('inbox'), { listen: false, toggle: false, onKey: (key) => run(key, true) });
// The plate's resting line, worded for the device in hand.
if (matchMedia('(hover: none) and (pointer: coarse)').matches) $('readout').textContent = 'tap any black key.';
const press = mountKeymap($('board'), $('readout'));

// The inbox only takes keys while you can see it.
let inboxInView = true;
new IntersectionObserver(([entry]) => (inboxInView = entry.isIntersecting), { threshold: 0.4 }).observe($('inbox'));

const INBOX_ACTS = new Set(['j', 'k', 'e', 'h', '#', 's', 'z']);
function run(act, tapped = false) {
  if (INBOX_ACTS.has(act) && (inboxInView || tapped)) {
    if (act === 'z' ? inbox.canUndo() : inbox.count()) inbox.act(act);
  }
  press(act);
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
const toggles = document.querySelectorAll('.keys-toggle');
toggles.forEach((t) =>
  t.addEventListener('click', () => {
    on = !on;
    toggles.forEach((x) => x.setAttribute('aria-pressed', String(on)));
  }),
);

// The plate's readout speaks to screen readers only while the plate is on screen.
new IntersectionObserver(([entry]) => $('readout').setAttribute('aria-live', entry.isIntersecting ? 'polite' : 'off'), { threshold: 0.3 }).observe($('keys'));

// The "Keymap" link glides down to the plate, unless motion is reduced.
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
