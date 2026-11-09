// A working miniature of Chord's inbox, driven by Chord's own keys: j/k move, e done, h snooze,
// # trash, s star, z undo. The mail is dev/seed-demo.py's invented inbox, split the way Chord
// splits it (Primary, Updates, Promotions).

const MAIL = [
  { split: 'primary', from: 'Ada Lovelace', subject: 'Engine notes, draft 3', snippet: "Here's where the Analytical Engine notes stand.", label: 'Projects/Chord', time: '9:10', unread: true },
  { split: 'primary', from: 'Mom', subject: 'Dinner Sunday?', snippet: "Are you two coming Sunday? I'm making the lemon chicken.", label: 'Friends', time: '8:32', unread: true },
  { split: 'primary', from: 'Grace Hopper, me', subject: 'Compiler talk', snippet: "Bring it! We'll have a projector and a very long extension cord.", time: '6:22', unread: true, count: 3 },
  { split: 'primary', from: 'Katherine Johnson', subject: 'Trajectory numbers', snippet: 'Attached are the corrected numbers for the second stage.', time: '4:22' },
  { split: 'primary', from: 'Alan Turing', subject: 'Lunch Thursday?', snippet: "There's a new place by the park that does excellent soup.", label: 'Friends', time: 'Nov 7', starred: true },
  { split: 'updates', from: 'Linear', subject: 'Your receipt from Linear', snippet: 'Receipt #2291 · $8.00 paid with Visa ending 4242', label: 'Receipts', time: '7:10' },
  { split: 'updates', from: 'GitHub', subject: '[chord] Split inbox rules (#4)', snippet: 'Merged #4 into main. 3 files changed.', time: '5:10', unread: true },
  { split: 'promotions', from: 'The Bike Shop', subject: 'Winter tune-ups are 20% off', snippet: 'Get your bike ready for the snow. Book before December 1.', time: '3:10' },
];
const SPLITS = [
  ['primary', 'Primary'],
  ['updates', 'Updates'],
  ['promotions', 'Promotions'],
];

const TOASTS = {
  e: 'Done. z to undo',
  h: 'Snoozed until tomorrow, 8:00. z to undo',
  '#': 'Moved to Trash. z to undo',
  z: 'Undone',
};

const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const still = matchMedia('(prefers-reduced-motion: reduce)');
const fresh = () => MAIL.map((m, i) => ({ ...m, id: i }));

// onKey: when given, the inbox's own key buttons call it instead of acting directly, so the page
// can run them through the same path as the keyboard.
export function mountInbox(root, { onKey } = {}) {
  let all = fresh();
  let split = 'primary';
  let sel = 0;
  const undo = [];
  const rows = () => all.filter((r) => r.split === split);

  root.innerHTML = `
    <div class="ib-head">
      <div class="ib-tabs" role="group" aria-label="Splits">${SPLITS.map(
        ([id, name]) => `<button type="button" data-split="${id}">${name} <i></i></button>`,
      ).join('')}</div>
      <span class="ib-acct">me@example.com</span>
    </div>
    <ol class="ib-list" aria-label="Inbox (demo)"></ol>
    <div class="ib-zero" hidden><b data-zero>Inbox zero.</b><button type="button" data-refill>Refill</button></div>
    <div class="ib-foot">
      <div class="ib-keys" role="toolbar" aria-label="Actions">
        <button type="button" data-key="j"><kbd>j</kbd><span>next</span></button>
        <button type="button" data-key="k"><kbd>k</kbd><span>prev</span></button>
        <button type="button" data-key="e"><kbd>e</kbd><span>done</span></button>
        <button type="button" data-key="h"><kbd>h</kbd><span>snooze</span></button>
        <button type="button" data-key="#"><kbd>#</kbd><span>trash</span></button>
        <button type="button" data-key="z"><kbd>z</kbd><span>undo</span></button>
      </div>
    </div>
    <p class="ib-toast" role="status" aria-live="polite"></p>`;

  const list = root.querySelector('.ib-list');
  const zero = root.querySelector('.ib-zero');
  const toast = root.querySelector('.ib-toast');
  let toastTimer;

  function render() {
    const rs = rows();
    list.innerHTML = rs
      .map(
        (r, i) => `<li class="ib-row${i === sel ? ' sel' : ''}${r.unread ? ' unread' : ''}" data-id="${r.id}">
          <span class="ib-dot"></span>
          <span class="ib-from">${esc(r.from)}${r.count ? ` <i>${r.count}</i>` : ''}</span>
          <span class="ib-subj"><b>${esc(r.subject)}</b> <span class="ib-snip">${esc(r.snippet)}</span></span>
          <span class="ib-label">${r.label ? esc(r.label) : ''}</span>
          <span class="ib-star">${r.starred ? '★' : ''}</span>
          <time>${r.time}</time>
        </li>`,
      )
      .join('');
    for (const tab of root.querySelectorAll('[data-split]')) {
      const id = tab.dataset.split;
      const unread = all.filter((r) => r.split === id && r.unread).length;
      tab.querySelector('i').textContent = unread || '';
      tab.setAttribute('aria-pressed', String(id === split));
    }
    zero.hidden = rs.length > 0;
    // Inbox zero only when every split is empty; otherwise just this one is clear.
    zero.querySelector('[data-zero]').textContent = all.length ? `${SPLITS.find(([id]) => id === split)[1]} clear.` : 'Inbox zero.';
    list.hidden = rs.length === 0;
  }

  function say(text) {
    toast.textContent = text;
    toast.classList.add('on');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove('on'), 2200);
  }

  // The row leaves, then the list closes the gap, as in Chord. Each step waits for its animation
  // but never longer than a beat, so a background tab can't leave the inbox stuck.
  const settle = (anim, ms) => Promise.race([anim.finished, new Promise((r) => setTimeout(r, ms))]).catch(() => {});
  async function leave(i) {
    const el = list.children[i];
    if (el && !still.matches) {
      await settle(el.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'translateX(-28px)' }], { duration: 140, easing: 'cubic-bezier(.4,0,1,1)', fill: 'forwards' }), 200);
      const h = el.offsetHeight;
      await settle(el.animate([{ height: `${h}px` }, { height: '0px', paddingBlock: '0px' }], { duration: 120, easing: 'cubic-bezier(0,0,.2,1)', fill: 'forwards' }), 180);
    }
  }

  async function step(key) {
    const rs = rows();
    if (key === 'r') {
      all = fresh();
      sel = 0;
      undo.length = 0;
      render();
      return say('Refilled');
    }
    if (key === 'j' || key === 'k') {
      if (!rs.length) return;
      sel = Math.max(0, Math.min(rs.length - 1, sel + (key === 'j' ? 1 : -1)));
      return render();
    }
    if (key === 'z') {
      const last = undo.pop();
      if (!last) return say('Nothing to undo');
      split = last.row.split;
      all.splice(last.at, 0, last.row);
      sel = rows().indexOf(last.row);
      render();
      return say(TOASTS.z);
    }
    if (!rs.length) return;
    const row = rs[sel];
    if (key === 's') {
      row.starred = !row.starred;
      render();
      return say(row.starred ? 'Starred' : 'Unstarred');
    }
    if (key === 'e' || key === 'h' || key === '#') {
      await leave(sel);
      const at = all.indexOf(row);
      all.splice(at, 1);
      undo.push({ row: { ...row, unread: false }, at });
      sel = Math.max(0, Math.min(sel, rows().length - 1));
      render();
      // At inbox zero the panel itself says so; no toast on top.
      if (rows().length) say(TOASTS[key]);
      else toast.classList.remove('on');
    }
  }

  // Actions queue, so a fast second key waits for the first instead of being dropped.
  let queue = Promise.resolve();
  let removing = 0; // queued e/h/# that haven't landed yet
  function act(key) {
    const removes = key === 'e' || key === 'h' || key === '#';
    if (removes) removing++;
    queue = queue.then(() => step(key)).catch(() => {}).finally(() => removes && removing--);
    return queue;
  }

  root.addEventListener('click', (ev) => {
    const b = ev.target.closest('[data-key]');
    if (b) return onKey ? onKey(b.dataset.key) : act(b.dataset.key);
    if (ev.target.closest('[data-refill]')) return act('r');
    const tab = ev.target.closest('[data-split]');
    if (tab) {
      split = tab.dataset.split;
      sel = 0;
      return render();
    }
    const li = ev.target.closest('.ib-row');
    if (li) {
      sel = [...list.children].indexOf(li);
      render();
    }
  });

  render();
  // Counts that include queued removals, so the page's readout tells the truth before they land.
  return { act, canUndo: () => undo.length + removing > 0, count: () => rows().length - removing };
}

// Copy the install command (without any prompt) from a button.
export function wireCopy(btn, source) {
  const label = btn.querySelector('[data-label]') ?? btn;
  btn.addEventListener('click', async () => {
    const cmd = source.textContent.replace(/^\$\s*/, '').trim();
    try {
      await navigator.clipboard.writeText(cmd);
      label.textContent = 'Copied';
    } catch {
      getSelection().selectAllChildren(source);
      label.textContent = 'Selected';
    }
    setTimeout(() => (label.textContent = 'Copy'), 1600);
  });
}
