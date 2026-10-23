// The audio Bibles: plays the chapter being read, marks each verse as it's read (and
// keeps it in view), starts at any verse, and carries on into the next chapter. A
// recording is downloaded when the reader asks for it (downloads.js): Listen on a book
// that isn't here yet offers that book or the whole Bible, and plays as soon as the
// chapter has arrived and been checked. Each chapter's verse timings come with its
// recording (`audio_chapter`).

import { audioChapter, audioUrl } from "./backend.js";
import { h, icon, replace } from "./dom.js";
import * as downloads from "./downloads.js";
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
  // A chapter the reader asked to hear that is still downloading, and where in it to
  // start: { s (as shown), at }
  waiting: null,
  // The verse element marked as being read, and its index in the timings
  heard: null,
  heardIndex: -1,
  userScrolled: 0,
  ui: null,
};
const cache = new Map();
let ctx = null;

const keyOf = (bible, book, chapter) => `${bible}|${book}|${chapter}`;
const firstNumber = (label) => parseInt(label, 10);

/** The recording of a chapter, or null: none, or none that can be had here (asked once
 * per chapter). */
async function infoFor(bible, book, chapter) {
  const key = keyOf(bible, book, chapter);
  if (!cache.has(key)) cache.set(key, audioChapter(bible, book, chapter).catch(() => null));
  const info = await cache.get(key);
  await downloads.loaded();
  return info && downloads.offered(info.recording) ? info : null;
}

/** "here", "coming" (downloading), "missing" (can be downloaded), or "none". */
const stateOf = (info) => downloads.chapterState(info.recording, info.code, info.chapter);

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
  audio.addEventListener("loadedmetadata", draw);
  document.body.append(audio);
  downloads.onChange(onDownloads);
  const ask = $("listen-ask");
  // A tap outside the question closes it
  ask.addEventListener("click", (event) => {
    if (event.target === ask) ask.close();
  });

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
  if (player.waiting?.s.key === key) return;
  const advancing = player.advancing;
  player.advancing = false;
  if (!advancing && !player.open) return;
  // The reader turned to another chapter, or the last one ended: the player goes with
  // them (playing on if it was playing, or waiting for it to download), or closes where
  // there's nothing to play
  const play = advancing || Boolean(player.waiting) || !player.audio.paused;
  const state = info ? stateOf(info) : "none";
  if (state === "here") load(player.shown, { play });
  else if (state === "coming" && play) wait(player.shown, 0);
  else close();
}

/** The verse bar's Listen button: shown when the chapter has a recording. */
export function syncVerseAction() {
  const action = document.querySelector('#verse-actions [data-action="listen"]');
  if (action) action.hidden = !player.shown?.info;
}

/** Play the chapter on screen from verse `n` (0: the beginning): at once if it's here,
 * once it arrives if it's downloading, or after asking to download it. */
export function playFrom(n) {
  const s = player.shown;
  if (!s?.info) return;
  const at = n ? startOf(s.info, n) : 0;
  const state = stateOf(s.info);
  if (state === "here") load(s, { at, play: true });
  else if (state === "coming") {
    // Waiting behind other books (the whole Bible downloading): it goes next
    const { r, d, book } = whereIn(s);
    if (d.book !== book.code) downloads.download(r.recording, { books: [book.code], from: { book: book.code, chapter: s.info.chapter } }).catch(() => {});
    wait(s, at);
  } else if (state === "missing") ask(s, at);
}

function startOf(info, n) {
  const v = info.verses.find(([label]) => firstNumber(label) >= n);
  return v ? v[1] : 0;
}

// ------------------------------------------------------------------ downloading

/** What of a recording `s` reads from, and the book it's in. */
function whereIn(s) {
  const r = downloads.recording(s.info.recording);
  const d = r?.download;
  return { r, d, book: d?.books.find((b) => b.code === s.info.code) };
}

/** Listen on a book that isn't here: download it, or the whole Bible, or not now. */
function ask(s, at) {
  const { r, d, book } = whereIn(s);
  if (!book) return;
  const dialog = $("listen-ask");
  const abbr = ctx.state.bibles.find((b) => b.id === s.bible)?.abbr ?? s.bible.toUpperCase();
  const go = async (books) => {
    dialog.close();
    try {
      await downloads.download(r.recording, { books, from: { book: book.code, chapter: s.info.chapter } });
      wait(s, at);
    } catch (error) {
      ctx.toast(`Couldn’t start the download: ${error?.message ?? error}`, 6000);
    }
  };
  const first = h("button", { type: "button", class: "button primary", onclick: () => go([book.code]) }, `Download ${book.name} · ${downloads.size(book.size - book.have)}`);
  replace(
    dialog,
    h("h2", { class: "ask-title", id: "listen-ask-title" }, `Listen to ${book.name}`),
    h("p", {}, `${r.reader}’s reading of the ${abbr} is downloaded when you want it, from the app’s page on GitHub.`),
    d.error ? h("p", { class: "ask-error" }, d.error) : null,
    h(
      "div",
      { class: "ask-actions" },
      first,
      h("button", { type: "button", class: "button", onclick: () => go(null) }, `${d.have ? "Download the rest of the Bible" : "Download the whole Bible"} · ${downloads.size(d.size - d.have)}`),
      h("button", { type: "button", class: "button", onclick: () => dialog.close() }, "Not now"),
    ),
    h("p", { class: "small muted" }, "GitHub sees your device’s address, as any website does. Every file is checked before it plays."),
  );
  dialog.showModal();
  first.focus();
}

/** Open the player on a chapter that's downloading, and play it once it's here. */
function wait(s, at) {
  player.audio.pause();
  player.loaded = null;
  player.heard?.classList.remove("is-heard");
  player.heard = null;
  player.heardIndex = -1;
  player.waiting = { s, at };
  openBar();
  onDownloads();
}

function onDownloads() {
  // The chapter in the player was deleted
  if (player.loaded && stateOf(player.loaded.info) !== "here") {
    close();
    return;
  }
  const w = player.waiting;
  if (w) {
    const state = stateOf(w.s.info);
    if (state === "here") {
      player.waiting = null;
      load(w.s, { at: w.at, play: true });
      return;
    }
    if (state !== "coming") {
      // The download stopped before this chapter came: cancelled, or it failed
      player.waiting = null;
      const error = whereIn(w.s).d?.error;
      ctx.toast(error ?? `${w.s.heading} wasn’t downloaded`, error ? 6000 : 2500);
      close();
      return;
    }
  }
  draw();
}

/** "12% of Psalms downloaded" (the number first: a phone shows only the start), and in
 * full, "Downloading Psalms: 3.9 MB of 32 MB". */
function waitingText(s) {
  const { d, book } = whereIn(s);
  if (!book) return ["Downloading…", ""];
  if (d.book !== book.code) return [`Waiting to download ${book.name}`, ""];
  const done = book.have + d.partial;
  return [`${Math.floor((100 * done) / book.size)}% of ${book.name} downloaded`, `Downloading ${book.name}: ${downloads.size(done)} of ${downloads.size(book.size)}`];
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
  if (player.waiting) return;
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

/** Stop and close the player. A download goes on (Settings, Audio Bibles, cancels it). */
function close() {
  player.audio.pause();
  player.open = false;
  player.loaded = null;
  player.waiting = null;
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
  // next one is here or on its way (a book not downloaded isn't fetched unasked)
  const next = view && s?.key === player.shown?.key ? view.next : null;
  const info = next ? await infoFor(s.bible, next.book, next.chapter) : null;
  if (info && ["here", "coming"].includes(stateOf(info))) {
    player.advancing = true;
    ctx.goTo(next.book, next.chapter, 0, { top: true });
  } else {
    draw();
  }
}

function onError() {
  const s = player.loaded;
  if (!s || !player.audio.getAttribute("src")) return;
  ctx.toast(`Couldn’t play ${s.heading}`);
  close();
  // A downloaded file may have gone (iOS clears its Caches folder when short of space):
  // then Listen offers to download it again
  downloads.refresh().then(() => chapterShown());
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
  const label = player.waiting ? "Plays once it has downloaded" : playing ? "Pause" : "Play";
  ui.play.setAttribute("aria-label", label);
  ui.play.title = label;
  ui.play.disabled = Boolean(player.waiting);
  for (const step of [ui.previous, ui.next, ui.seek]) step.disabled = Boolean(player.waiting);
  const s = player.loaded ?? player.waiting?.s;
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
  if (player.waiting) {
    [ui.sub.textContent, ui.sub.title] = waitingText(player.waiting.s);
    ui.seek.value = "0";
    return;
  }
  ui.sub.title = "";
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
