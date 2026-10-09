// The audio Bibles: plays the chapter being read, marks each verse as it's read (and
// keeps it in view), starts at any verse, and carries on into the next chapter. The
// recordings ship with the app, one file per chapter; nothing is downloaded. Each
// chapter's verse timings come with its recording (`audio_chapter`).

import { audioChapter, audioUrl } from "./backend.js";
import { h, icon, replace } from "./dom.js";
import { AUDIO_SPEEDS } from "./settings.js";

const $ = (id) => document.getElementById(id);

// How long after the reader scrolls by hand the page stops following along
const HANDS_OFF_MS = 5000;

const player = {
  audio: null,
  // The chapter on screen and its recording (info null: none): { key, info, bible, book, chapter, heading }
  shown: null,
  // The chapter loaded into the player (same shape)
  loaded: null,
  open: false,
  // The chapter ended and the next is being opened: play it when it's shown
  advancing: false,
  // The verse element marked as being read, and its index in the timings
  heard: null,
  heardIndex: -1,
  userScrolled: 0,
  // Recordings whose files aren't in this build (a development build, say), and those
  // a chapter of has played: their files are there
  missing: new Set(),
  found: new Set(),
  ui: null,
};
const cache = new Map();
let ctx = null;

const keyOf = (bible, book, chapter) => `${bible}|${book}|${chapter}`;
const firstNumber = (label) => parseInt(label, 10);

/** The recording of a chapter, or null (asked once per chapter). */
function infoFor(bible, book, chapter) {
  const key = keyOf(bible, book, chapter);
  if (!cache.has(key)) cache.set(key, audioChapter(bible, book, chapter).catch(() => null));
  return cache.get(key).then((info) => (info && !player.missing.has(info.recording) ? info : null));
}

/** "4:05", "1:02:10" */
function clock(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  const hms = [Math.floor(s / 3600), Math.floor((s % 3600) / 60), s % 60];
  return hms[0] ? `${hms[0]}:${String(hms[1]).padStart(2, "0")}:${String(hms[2]).padStart(2, "0")}` : `${hms[1]}:${String(hms[2]).padStart(2, "0")}`;
}

const speedLabel = (s) => `${s}×`;

// ------------------------------------------------------------------ setup

export function initListen(context) {
  ctx = context;
  const audio = document.createElement("audio");
  audio.preload = "auto";
  player.audio = audio;
  audio.addEventListener("timeupdate", onTime);
  audio.addEventListener("seeked", onTime);
  audio.addEventListener("play", draw);
  audio.addEventListener("pause", draw);
  audio.addEventListener("ended", onEnded);
  audio.addEventListener("error", onError);
  audio.addEventListener("loadedmetadata", () => {
    if (player.loaded) player.found.add(player.loaded.info.recording);
    draw();
  });
  document.body.append(audio);

  const button = $("listen-button");
  button.append(icon("listen"));
  button.addEventListener("click", () => {
    if (player.open && player.loaded?.key === player.shown?.key) togglePlay();
    else playFrom(0);
  });

  // Following along stops for a while once the reader scrolls by hand
  const reader = $("reader");
  const hands = () => (player.userScrolled = Date.now());
  for (const type of ["wheel", "touchmove", "keydown", "mousedown"]) reader.addEventListener(type, hands, { passive: true });

  buildBar();
  mediaSession();
}

function buildBar() {
  const bar = $("player");
  const ui = {
    play: h("button", { type: "button", class: "icon-btn player-play", onclick: () => togglePlay() }),
    previous: h("button", { type: "button", class: "icon-btn", "aria-label": "Previous verse", title: "Previous verse", onclick: () => stepVerse(-1) }, icon("previous")),
    next: h("button", { type: "button", class: "icon-btn", "aria-label": "Next verse", title: "Next verse", onclick: () => stepVerse(1) }, icon("next")),
    title: h("span", { class: "player-title" }),
    sub: h("span", { class: "player-sub" }),
    speed: h("button", { type: "button", class: "player-speed", title: "Reading speed", onclick: () => nextSpeed() }),
    close: h("button", { type: "button", class: "icon-btn", "aria-label": "Stop and close the player", title: "Stop", onclick: () => close() }, icon("close")),
    seek: h("input", { type: "range", class: "player-seek", min: "0", max: "1", step: "1", value: "0", "aria-label": "Position in the chapter" }),
  };
  ui.seek.addEventListener("input", () => {
    player.audio.currentTime = Number(ui.seek.value);
  });
  replace(
    bar,
    h("div", { class: "player-controls" }, ui.previous, ui.play, ui.next),
    h("div", { class: "player-what" }, ui.title, ui.sub),
    ui.speed,
    ui.close,
    ui.seek,
  );
  player.ui = ui;
  draw();
}

// ------------------------------------------------------------------ the chapter on screen

/** Called whenever a chapter is drawn: shows the Listen button if it has a recording,
 * and keeps the player on the chapter being read. */
export async function chapterShown() {
  const view = ctx.state.chapter;
  if (!view) return;
  const bible = ctx.settings.translation;
  const key = keyOf(bible, view.book, view.chapter);
  const info = await infoFor(bible, view.book, view.chapter);
  if (ctx.state.chapter !== view) return; // the reader moved on meanwhile
  player.shown = { key, info, bible, book: view.book, chapter: view.chapter, heading: view.heading };
  const button = $("listen-button");
  button.hidden = !info;
  if (info) {
    button.setAttribute("aria-label", `Listen to ${view.heading}, read by ${info.reader}`);
    button.title = `Listen (read by ${info.reader})`;
  }
  syncVerseAction();

  if (player.loaded?.key === key) {
    // The same chapter drawn again (a setting changed): mark the verse again
    player.heard = null;
    player.heardIndex = -1;
    onTime();
    return;
  }
  if (player.advancing) {
    player.advancing = false;
    if (info) load(player.shown, { play: true });
    else close();
    return;
  }
  if (player.open) {
    // The reader turned to another chapter: the player goes with them (playing on if
    // it was playing), or closes where there's no recording
    if (info) load(player.shown, { play: !player.audio.paused });
    else close();
  }
}

/** The verse bar's Listen button: shown when the chapter has a recording. */
export function syncVerseAction() {
  const action = document.querySelector('#verse-actions [data-action="listen"]');
  if (action) action.hidden = !player.shown?.info;
}

/** Play the chapter on screen from verse `n` (0: the beginning). */
export function playFrom(n) {
  const s = player.shown;
  if (!s?.info) return;
  load(s, { at: n ? startOf(s.info, n) : 0, play: true });
}

function startOf(info, n) {
  const v = info.verses.find(([label]) => firstNumber(label) >= n);
  return v ? v[1] : 0;
}

// ------------------------------------------------------------------ playing

function load(s, { at = 0, play = false } = {}) {
  const a = player.audio;
  const url = audioUrl(s.info.file);
  const fresh = player.loaded?.key !== s.key;
  player.loaded = s;
  player.heard?.classList.remove("is-heard");
  player.heard = null;
  player.heardIndex = -1;
  const go = () => {
    a.playbackRate = ctx.settings.audio.speed;
    if (Math.abs(a.currentTime - at) > 0.05) a.currentTime = at;
    if (play) a.play().catch((error) => onPlayFailed(error));
    draw();
  };
  if (fresh || a.dataset.url !== url) {
    a.dataset.url = url;
    a.src = url;
    a.defaultPlaybackRate = ctx.settings.audio.speed;
    a.addEventListener("loadedmetadata", go, { once: true });
    a.load();
  } else {
    go();
  }
  openBar();
  setMediaMetadata();
}

function togglePlay() {
  const a = player.audio;
  if (!player.loaded) return playFrom(0);
  if (a.paused) a.play().catch((error) => onPlayFailed(error));
  else a.pause();
}

function stepVerse(direction) {
  const info = player.loaded?.info;
  if (!info) return;
  const i = Math.max(0, Math.min(info.verses.length - 1, currentIndex() + direction));
  player.audio.currentTime = info.verses[i][1];
  if (player.audio.paused) onTime();
}

function nextSpeed() {
  const s = ctx.settings.audio.speed;
  const next = AUDIO_SPEEDS[(AUDIO_SPEEDS.indexOf(s) + 1) % AUDIO_SPEEDS.length];
  ctx.changeSettings((x) => {
    x.audio.speed = next;
  });
  player.audio.playbackRate = next;
  player.audio.defaultPlaybackRate = next;
  draw();
}

function close() {
  player.audio.pause();
  player.open = false;
  player.loaded = null;
  player.advancing = false;
  player.heard?.classList.remove("is-heard");
  player.heard = null;
  player.heardIndex = -1;
  $("player").hidden = true;
  $("app").dataset.player = "closed";
  draw();
}

function openBar() {
  player.open = true;
  $("player").hidden = false;
  $("app").dataset.player = "open";
}

async function onEnded() {
  const s = player.loaded;
  const view = ctx.state.chapter;
  // Carry on into the next chapter, if it's the chapter on screen that ended and the
  // next one has a recording too
  const next = view && s?.key === player.shown?.key ? view.next : null;
  if (next && (await infoFor(s.bible, next.book, next.chapter))) {
    player.advancing = true;
    ctx.goTo(next.book, next.chapter, 0, { top: true });
  } else {
    draw();
  }
}

function onError() {
  const s = player.loaded;
  if (!s || !player.audio.getAttribute("src")) return;
  if (player.found.has(s.info.recording)) {
    ctx.toast(`Couldn’t play ${s.heading}`);
  } else {
    // The recording's files aren't in this build: hide it rather than fail again
    player.missing.add(s.info.recording);
    ctx.toast("This recording isn’t included in this build of the app");
  }
  close();
  chapterShown();
}

function onPlayFailed(error) {
  // A browser may refuse to start playing without a tap; nothing to report then
  if (error?.name !== "NotAllowedError") ctx.toast(`Couldn’t play the recording: ${error?.message ?? error}`);
  draw();
}

// ------------------------------------------------------------------ following along

/** The index of the verse being read now. */
function currentIndex() {
  const info = player.loaded?.info;
  if (!info) return -1;
  const t = player.audio.currentTime + 0.05;
  let lo = 0;
  let hi = info.verses.length - 1;
  let found = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (info.verses[mid][1] <= t) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

function onTime() {
  drawTime();
  const s = player.loaded;
  if (!s || s.key !== player.shown?.key) return;
  const i = currentIndex();
  if (i === player.heardIndex && player.heard?.isConnected) return;
  player.heardIndex = i;
  player.heard?.classList.remove("is-heard");
  const label = s.info.verses[i]?.[0];
  const el = label === undefined ? null : $("reader").querySelector(`#v${firstNumber(label)}`);
  player.heard = el;
  if (!el) return;
  el.classList.add("is-heard");
  setMediaMetadata();
  if (ctx.settings.audio.follow && !player.audio.paused && Date.now() - player.userScrolled > HANDS_OFF_MS) {
    const box = el.getBoundingClientRect();
    const view = $("reader").getBoundingClientRect();
    const bottomRoom = $("player").getBoundingClientRect().top;
    if (box.top < view.top + 8 || box.bottom > Math.min(view.bottom, bottomRoom) - 8) {
      el.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }
}

// ------------------------------------------------------------------ drawing

function draw() {
  const ui = player.ui;
  if (!ui) return;
  const a = player.audio;
  const playing = player.loaded && !a.paused;
  replace(ui.play, icon(playing ? "pause" : "play"));
  ui.play.setAttribute("aria-label", playing ? "Pause" : "Play");
  ui.play.title = playing ? "Pause" : "Play";
  const s = player.loaded;
  ui.title.textContent = s ? s.heading : "";
  ui.speed.textContent = speedLabel(ctx.settings.audio.speed);
  ui.speed.setAttribute("aria-label", `Reading speed ${speedLabel(ctx.settings.audio.speed)}; change`);
  const listen = $("listen-button");
  listen.classList.toggle("is-playing", Boolean(playing && s?.key === player.shown?.key));
  drawTime();
}

function drawTime() {
  const ui = player.ui;
  if (!ui) return;
  const a = player.audio;
  const s = player.loaded;
  const duration = Number.isFinite(a.duration) ? a.duration : s?.info.duration ?? 0;
  ui.sub.textContent = s ? `${s.info.reader} · ${clock(a.currentTime)} / ${clock(duration)}` : "";
  ui.seek.max = String(Math.max(1, Math.round(duration)));
  if (document.activeElement !== ui.seek) ui.seek.value = String(Math.round(a.currentTime));
  if ("mediaSession" in navigator && Number.isFinite(a.duration) && a.duration > 0) {
    try {
      navigator.mediaSession.setPositionState({ duration: a.duration, playbackRate: a.playbackRate, position: Math.min(a.currentTime, a.duration) });
    } catch {}
  }
}

// ------------------------------------------------------------------ system controls

/** Headphone buttons, the lock screen, and the system's media controls. */
function mediaSession() {
  if (!("mediaSession" in navigator)) return;
  const on = (action, fn) => {
    try {
      navigator.mediaSession.setActionHandler(action, fn);
    } catch {}
  };
  on("play", () => togglePlay());
  on("pause", () => player.audio.pause());
  on("previoustrack", () => stepVerse(-1));
  on("nexttrack", () => stepVerse(1));
  on("seekto", (d) => {
    if (Number.isFinite(d.seekTime)) player.audio.currentTime = d.seekTime;
  });
}

function setMediaMetadata() {
  const s = player.loaded;
  if (!s || !("mediaSession" in navigator) || typeof MediaMetadata === "undefined") return;
  const label = s.info.verses[player.heardIndex]?.[0];
  navigator.mediaSession.metadata = new MediaMetadata({
    title: label && label !== "0" ? `${s.heading}:${label}` : s.heading,
    artist: s.info.reader,
    album: "Scriptorium",
  });
}
