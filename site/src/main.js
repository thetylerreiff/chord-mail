// Chord's site: the exploded keypress runs on e, and c copies the installer.

const fig = document.getElementById("fig");
const stack = document.getElementById("stack");
const key = document.getElementById("key");
const pulse = document.getElementById("pulse");
const still = matchMedia("(prefers-reduced-motion: reduce)");
const sleep = ms => new Promise(r => setTimeout(r, still.matches ? Math.min(ms, 400) : ms));
const move = (el, frames, ms) =>
  still.matches ? Promise.resolve() : el.animate(frames, { duration: ms, easing: "cubic-bezier(.3,0,.2,1)" }).finished;

// One run: the key goes down, the cache changes at once, and Gmail
// catches up a beat later. Then it all resets.
let running = false;
async function press() {
  if (running) return;
  running = true;
  fig.classList.add("pressed");
  await move(key, [{ transform: "translateY(0)" }, { transform: "translateY(16px)" }, { transform: "translateY(0)" }], 240);
  await move(pulse, [{ transform: "translateY(300px)", opacity: 1 }, { transform: "translateY(446px)", opacity: 1 }], 200);
  fig.classList.add("local");
  await sleep(900);
  await move(pulse, [{ transform: "translateY(446px)", opacity: 1 }, { transform: "translateY(656px)", opacity: 1 }, { transform: "translateY(656px)", opacity: 0 }], 560);
  fig.classList.add("synced");
  await sleep(2400);
  fig.classList.remove("pressed", "local", "synced");
  await sleep(500);
  running = false;
}

// Copy the install command, without the prompt.
const copyText = document.getElementById("copy-text");
async function copy() {
  const cmd = document.getElementById("install-cmd").textContent.replace(/^\$\s*/, "");
  try {
    await navigator.clipboard.writeText(cmd);
    copyText.textContent = "Copied";
  } catch {
    getSelection().selectAllChildren(document.getElementById("install-cmd"));
    copyText.textContent = "Selected";
  }
  setTimeout(() => (copyText.textContent = "Copy"), 1600);
}

document.getElementById("copy").addEventListener("click", copy);
document.getElementById("run").addEventListener("click", press);
key.addEventListener("click", press);
document.getElementById("chip").addEventListener("click", press);
addEventListener("keydown", e => {
  if (e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
  if (e.key === "e" || e.key === "E") press();
  else if (e.key === "c" || e.key === "C") copy();
});

// Narrow screens swap the callouts for a list, so crop to the stack.
const narrow = matchMedia("(max-width: 980px)");
const fit = () => stack.setAttribute("viewBox", narrow.matches ? "-350 100 700 700" : "-370 96 1130 720");
narrow.addEventListener("change", fit);
fit();

// The ring turns unless motion is reduced.
const ring = () => (still.matches ? stack.pauseAnimations() : stack.unpauseAnimations());
still.addEventListener("change", ring);
ring();

if (!still.matches) setTimeout(press, 900);
