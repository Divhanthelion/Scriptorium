// The audio Bibles' downloads: what of each recording is on this device, and downloading
// it when the reader asks (the app fetches nothing otherwise). The Rust side does the
// work (kjv_audio), checking every chapter against the SHA-256 built into the app; this
// keeps the page's picture of it current, asking again twice a second while a download
// runs.

import { audioCancel, audioDelete, audioDownload, audioDownloads } from "./backend.js";

let list = [];
let first = null;
let timer = null;
const listeners = new Set();

/** Ask the Rust side what's here; again every half second while anything downloads. */
export async function refresh() {
  try {
    list = await audioDownloads();
  } catch {
    // Keep what was known: the next refresh may answer
  }
  for (const fn of listeners) fn();
  clearTimeout(timer);
  if (list.some((r) => r.download?.running)) timer = setTimeout(refresh, 500);
  return list;
}

/** The recordings, once they've been asked for. */
export function loaded() {
  first ??= refresh();
  return first;
}

export function recordings() {
  return list;
}

export function recording(id) {
  return list.find((r) => r.recording === id) ?? null;
}

/** Call `fn` whenever what's downloaded changes; returns a function that stops it. */
export function onChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** Chapter `n` of book `code` of a recording: "here" (it can play), "coming" (it's being
 * downloaded), "missing" (it can be downloaded), or "none" (it can't be had). */
export function chapterState(id, code, n) {
  const r = recording(id);
  if (!r) return "none";
  if (r.included) return "here";
  const d = r.download;
  const book = d?.books.find((b) => b.code === code);
  if (!book) return "none";
  if (book.chapters[n - 1] === "1") return "here";
  return d.running && (d.book === code || d.queued.includes(code)) ? "coming" : "missing";
}

/** Whether a recording can be heard here at all: it came with the app, or it can be
 * downloaded. */
export function offered(id) {
  const r = recording(id);
  return Boolean(r && (r.included || r.download));
}

/** Download `books` (codes; null: all) of a recording, `from` ({ book, chapter }) first. */
export async function download(id, { books = null, from = null } = {}) {
  await audioDownload({ recording: id, books, from });
  return refresh();
}

export async function cancel(id) {
  await audioCancel(id);
  return refresh();
}

export async function remove(id) {
  await audioDelete(id);
  return refresh();
}

/** "529 MB", "25 MB", "0.2 MB", "1.3 GB" */
export function size(bytes) {
  const mb = bytes / 1e6;
  if (mb >= 1000) return `${(mb / 1000).toFixed(1)} GB`;
  if (mb >= 10) return `${Math.round(mb)} MB`;
  return `${Math.max(0.1, mb).toFixed(1)} MB`;
}

/** "12 of 66 books": books wholly here. */
export function booksHere(d) {
  return d.books.filter((b) => b.have === b.size).length;
}
