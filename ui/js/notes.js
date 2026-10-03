// The Commentary panel: the chosen commentaries' notes on the selected verse, or, with
// no verse selected, one commentary read through the whole chapter, in whatever
// translation is being read. Notes are keyed to the KJV; Rust maps the verse first.

import { call } from "./backend.js";
import { h, icon, replace } from "./dom.js";

let catalogue = null; // [{ id, name, author, year, tradition, coverage, credit, about }]
let seq = 0;
let shown = null; // the place the panel shows (see place())
// A note to show and open (from a search result): { id, label, query, at }; `at` is
// the place it was shown at, so it's let go once the reader moves on
let focus = null;

/** Notes longer than this many characters start folded (on a verse; read through a
 * chapter, they're open). */
const FOLD = 2400;
// The chapter and commentary last read through, to start at the top of the next
let lastRead = null;

async function commentaries() {
  catalogue ??= await call("commentaries");
  return catalogue;
}

/** The commentaries to show: the reader's choice, or all of them. */
export function chosen(ctx, all) {
  const picked = ctx.settings.commentaries;
  return picked ? all.filter((c) => picked.includes(c.id)) : all;
}

// ------------------------------------------------------------------ note markup

/** USFM code -> the app's book key, from the KJV's book list. */
function bookByCode(ctx) {
  const kjv = ctx.state.bibles.find((b) => b.id === "kjv");
  return new Map((kjv?.books ?? []).map((b) => [b.code, b.name]));
}

function codeOf(ctx, book) {
  for (const [code, name] of bookByCode(ctx)) if (name === book) return code;
  return null;
}

/** A commentary's name for buttons and lists: "Matthew Henry", "Tyndale" */
function shortName(c) {
  return c.short ?? c.name;
}

function list(names) {
  return names.length < 3 ? names.join(" and ") : `${names.slice(0, -1).join(", ")}, and ${names.at(-1)}`;
}

/** Open a reference ("JHN.3.16-JHN.3.18 ROM.5.8": the first place), in the
 * translation being read (notes cite KJV numbering). */
async function follow(ctx, to) {
  const first = to.split(/\s+/)[0].split("-")[0];
  const [code, chapter, verse] = first.split(".");
  const book = bookByCode(ctx).get(code);
  if (!book) return;
  const c = Number(chapter) || 1;
  const v = Number(verse) || 0;
  const translation = ctx.settings.translation;
  if (translation !== "kjv") {
    try {
      const at = await call("bible_map", { from: "kjv", to: translation, book, chapter: c, verse: v });
      if (at) return ctx.goTo(at.book, at.chapter, parseInt(at.verse, 10) || 0, { fromPanel: true });
    } catch {
      // fall through to the same numbers
    }
  }
  ctx.goTo(book, c, v, { fromPanel: true });
}

/** "1JN.4.10" -> "1 John 4:10", "2CO.5.19-2CO.5.21" -> "2 Corinthians 5:19-21" */
function describe(ctx, target) {
  const [first, last] = target.split("-");
  const [code, chapter, verse] = first.split(".");
  const book = bookByCode(ctx).get(code);
  if (!book) return target;
  let text = verse ? ctx.reference(book, Number(chapter), verse) : ctx.heading(book, Number(chapter));
  if (last) {
    const [code2, chapter2, verse2] = last.split(".");
    text += code2 === code && chapter2 === chapter ? `-${verse2}` : `-${chapter2}:${verse2}`;
  }
  return text;
}

function link(ctx, target, label) {
  return h("button", { type: "button", class: "text-link note-link", title: `Open ${describe(ctx, target)}`, onclick: () => follow(ctx, target) }, label);
}

/** "Numb 1:22", "2Chron 35:15", "26:14" */
const PLACE = /(?:(?:[1-3]\s?)?[A-Z][a-z]*\.?\s+)?\d+:\d+(?:-\d+(?::\d+)?)?/g;

/** A reference to one place or several. The Treasury lists several under one
 * reference ("Lu 2:14; Ro 5:8; 1Jo 4:9,10,19"), as Wesley does ("Numb 1:22 26:14");
 * when the text has a part for each place, in order, each part opens its own. */
function references(node, ctx, kids) {
  const targets = node.getAttribute("to").split(/\s+/).filter(Boolean);
  const onlyText = [...node.childNodes].every((n) => n.nodeType === Node.TEXT_NODE);
  if (targets.length > 1 && onlyText) {
    const text = node.textContent;
    // Parts between ";" and ",", any annotation ("*title", "&c.") after them
    const pieces = text.split(/([;,]\s*)/); // part, separator, part, ...
    const placed = pieces.map((t, i) => i % 2 === 0 && /\d/.test(t));
    const count = placed.filter(Boolean).length;
    const lastPlaced = placed.lastIndexOf(true);
    if (count === targets.length && pieces.slice(0, lastPlaced + 1).every((t, i) => i % 2 === 1 || placed[i])) {
      let k = 0;
      return pieces.map((t, i) => (placed[i] ? link(ctx, targets[k++], t) : t));
    }
    // Places separated by spaces
    const found = [...text.matchAll(PLACE)];
    if (found.length === targets.length && /^[\s;,.]*$/.test(text.replace(PLACE, ""))) {
      const out = [];
      let at = 0;
      found.forEach((m, k) => {
        out.push(text.slice(at, m.index), link(ctx, targets[k], m[0]));
        at = m.index + m[0].length;
      });
      out.push(text.slice(at));
      return out;
    }
  }
  return link(ctx, targets[0], kids());
}

/** A line's or list item's indent below the first: " level-2" */
function level(node) {
  const n = node.getAttribute("level");
  return n === "2" || n === "3" ? ` level-${n}` : "";
}

/** The library's note markup (docs/LIBRARY.md) as page elements. Only the known
 * elements are drawn; nothing in a note is ever treated as HTML. */
export function renderMarkup(body, ctx) {
  const doc = new DOMParser().parseFromString(`<n>${body}</n>`, "application/xml");
  if (doc.querySelector("parsererror")) {
    // Never expected (the importer checks every note); show the words, not an error
    const words = body.replace(/<[^>]*>/g, " ").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
    return [h("p", null, words.replace(/\s+/g, " ").trim())];
  }
  const convert = (node) => {
    if (node.nodeType === Node.TEXT_NODE) return node.nodeValue;
    if (node.nodeType !== Node.ELEMENT_NODE) return null;
    const kids = () => [...node.childNodes].map(convert);
    switch (node.nodeName) {
      case "p":
        return h("p", null, kids());
      case "h":
        return h("h4", { class: "note-heading" }, kids());
      case "l":
        return h("div", { class: `note-line${level(node)}` }, kids());
      case "li":
        return h("div", { class: `note-item${level(node)}` }, kids());
      case "tr":
        return h("div", { class: "note-row" }, kids());
      case "td":
        return h("span", { class: "note-cell" }, kids());
      case "i":
        return h("em", null, kids());
      case "b":
        return h("strong", null, kids());
      case "sup":
        return h("sup", null, kids());
      case "sub":
        return h("sub", null, kids());
      case "sc":
        return h("span", { class: "sc" }, kids());
      case "lang":
        return h("span", { lang: node.getAttribute("code") }, kids());
      case "fn":
        return h("span", { class: "note-fn" }, " [", kids(), "]");
      case "br":
        return h("br");
      case "ref":
        return node.getAttribute("to")?.trim() ? references(node, ctx, kids) : h("span", null, kids());
      default:
        return h("span", null, kids());
    }
  };
  return [...doc.documentElement.childNodes].map(convert);
}

// ------------------------------------------------------------------ panel

/**
 * Show commentary `id`'s note labelled `label` (as a search result gives it), placed in
 * the KJV's numbering at `book` `chapter`:`verse` (chapter 0: the book's introduction;
 * verse 0: the chapter's), in the translation being read, with the Commentary panel open.
 */
export async function openNote(ctx, { id, label, nth = 0, book, chapter, verse }) {
  focus = { id, label, nth, at: null };
  const c = chapter || 1;
  const v = chapter ? verse : 0;
  let at = { book, chapter: c, verse: v };
  const translation = ctx.settings.translation;
  if (translation !== "kjv") {
    try {
      const mapped = await call("bible_map", { from: "kjv", to: translation, book, chapter: c, verse: v });
      if (mapped) at = { book: mapped.book, chapter: mapped.chapter, verse: v ? parseInt(mapped.verse, 10) || 0 : 0 };
    } catch {
      // the same numbers
    }
  }
  // The panel stays open, turning to the note (closing it on a phone, to open it again,
  // would go back in history after the notes were open, and close them)
  await ctx.goTo(at.book, at.chapter, at.verse);
  ctx.openPanel("notes", { focus: false });
}

/** The commentary read through a chapter: the one a search result opened, the one
 * read last, or the first chosen. */
function readingId(ctx, all) {
  const known = (id) => all.some((c) => c.id === id);
  if (focus && known(focus.id)) return focus.id;
  if (known(ctx.settings.reading)) return ctx.settings.reading;
  return chosen(ctx, all)[0]?.id ?? all[0]?.id ?? null;
}

/** One commentary read through the chapter: its notes in order, open, with the
 * chapters either side to read on. */
function drawReading(body, ctx, all, found, view, id, heading) {
  const c = all.find((x) => x.id === id);
  const result = found.commentaries[0];
  const code = codeOf(ctx, view.book);
  let content;
  if (!c) content = h("p", { class: "empty" }, "No commentaries.");
  else if (!result?.notes.length) {
    content = h(
      "p",
      { class: "empty" },
      code && !c.books.includes(code) ? `${shortName(c)} has nothing on ${ctx.bookName(view.book)}.` : `${shortName(c)} has nothing on ${heading}.`,
    );
  } else {
    content = h(
      "section",
      { class: "commentary reading", "data-commentary": c.id },
      h("h3", { class: "commentary-name" }, c.name, h("span", { class: "commentary-meta" }, ` · ${c.author} · ${c.tradition}`)),
      result.notes.map((n) => note(n, ctx, { reading: true })),
      h("p", { class: "commentary-credit" }, result.credit),
    );
  }
  replace(
    body,
    h("p", { class: "notes-where" }, c ? `${shortName(c)} on ${heading}` : heading),
    !found.same && found.kjv ? h("p", { class: "notes-kjv" }, `The commentaries number it as the KJV does: ${found.kjv}.`) : null,
    readChooser(ctx, all, id),
    content,
    chapterNav(ctx, view),
    h("p", { class: "notes-hint" }, "Select a verse to see just its notes, from the commentaries you choose."),
  );
  // A new chapter, or another commentary, starts at the top
  const key = `${view.book}/${view.chapter}/${id}`;
  if (key !== lastRead) body.scrollTop = 0;
  lastRead = key;
}

/** Which commentary to read through the chapter, one at a time (a menu: eleven chips
 * would push the text off a phone's screen). */
function readChooser(ctx, all, id) {
  const select = h(
    "select",
    {
      class: "notes-read-select",
      "aria-label": "Commentary to read",
      onchange: () => {
        focus = null;
        ctx.changeSettings((s) => { s.reading = select.value; });
        ctx.refreshPanel();
      },
    },
    all.map((c) => h("option", { value: c.id, selected: c.id === id }, `${shortName(c)} · ${c.tradition}`)),
  );
  return h("div", { class: "field notes-read-pick" }, select);
}

/** The chapters either side: the reader turns to them, and the panel follows. */
function chapterNav(ctx, view) {
  const go = (to) => () => ctx.goTo(to.book, to.chapter, 0, { top: true });
  return h(
    "nav",
    { class: "chapter-nav notes-nav", "aria-label": "Chapters" },
    view.prev ? h("button", { type: "button", onclick: go(view.prev) }, icon("chevronLeft"), h("span", {}, ctx.heading(view.prev.book, view.prev.chapter))) : h("span"),
    view.next ? h("button", { type: "button", onclick: go(view.next) }, h("span", {}, ctx.heading(view.next.book, view.next.chapter)), icon("chevronRight")) : h("span"),
  );
}

function place(ctx) {
  const view = ctx.state.chapter;
  return view ? [ctx.settings.translation, view.book, view.chapter, ctx.state.selectedVerse ?? 0].join("/") : null;
}

/** Whether the reader has moved since the panel was drawn. */
export function notesStale(ctx) {
  return place(ctx) !== shown;
}

function chooser(ctx, all) {
  const picked = new Set(chosen(ctx, all).map((c) => c.id));
  return h(
    "div",
    { class: "chips", role: "group", "aria-label": "Commentaries to show" },
    all.map((c) =>
      h(
        "button",
        {
          type: "button",
          class: "chip",
          "aria-pressed": String(picked.has(c.id)),
          title: `${c.name} (${c.tradition})`,
          onclick: () => {
            const next = new Set(picked);
            next.has(c.id) ? next.delete(c.id) : next.add(c.id);
            ctx.changeSettings((s) => {
              s.commentaries = all.filter((x) => next.has(x.id)).map((x) => x.id);
            });
            ctx.refreshPanel();
          },
        },
        shortName(c),
      ),
    ),
  );
}

function note(n, ctx, { reading = false } = {}) {
  const long = n.body.length > FOLD;
  const label = n.label;
  const content = h("div", { class: "note-body" }, renderMarkup(n.body, ctx));
  const place = h("span", { class: "note-place" }, n.label);
  // Read through a chapter, every note is open but one begun in an earlier chapter,
  // which was read there
  const folded = reading ? n.earlier : long;
  if (!folded) return h("section", { class: "note", "data-label": label }, h("h4", { class: "note-label" }, place), content);
  // A folded article is named by its title ("Adam and Eve"), where it opens with one
  const title = /^<h>(.*?)<\/h>/.exec(n.body)?.[1].replace(/<br\/>/g, " ").replace(/<sc>(.*?)<\/sc>/g, (_, x) => x.toUpperCase()).replace(/<[^>]*>/g, "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
  return h(
    "details",
    { class: "note", "data-label": label },
    h(
      "summary",
      { class: "note-label" },
      title ? `${title} · ` : null,
      place,
      h("span", { class: "muted" }, n.earlier ? " · begun in an earlier chapter" : ` · ${Math.round(n.body.length / 1000)}k characters`),
    ),
    content,
  );
}

export async function renderNotes(body, ctx) {
  const view = ctx.state.chapter;
  if (!view) return;
  const verse = ctx.state.selectedVerse ?? 0;
  // No verse selected: one commentary read through the whole chapter
  const reading = !verse;
  const heading = ctx.heading(view.book, view.chapter);
  const where = verse ? ctx.reference(view.book, view.chapter, verse) : heading;
  const mine = ++seq;
  // This drawing is still wanted: no later one, and the panel still shows notes
  const current = () => mine === seq && ctx.state.panel === "notes";
  shown = place(ctx);
  // Notes usually arrive at once; only say they're loading when they don't
  const loading = setTimeout(() => {
    if (current()) replace(body, h("p", { class: "status" }, `Loading notes on ${where}…`));
  }, 150);
  let all;
  let found; // { kjv, same, commentaries }
  let readId = null;
  if (focus?.at && focus.at !== shown) focus = null;
  try {
    all = await commentaries();
    let ids;
    if (reading) {
      readId = readingId(ctx, all);
      ids = readId ? [readId] : [];
    } else {
      ids = chosen(ctx, all).map((c) => c.id);
      // A note opened from a search result shows even if its commentary isn't chosen
      if (focus && !ids.includes(focus.id)) ids.push(focus.id);
    }
    found = ids.length
      ? await call("notes", { commentaries: ids, bible: ctx.settings.translation, book: view.book, chapter: view.chapter, verse, whole: reading })
      : { kjv: "", same: true, commentaries: [] };
  } catch (error) {
    clearTimeout(loading);
    if (current()) replace(body, h("p", { class: "chat-error" }, `Couldn’t load the notes: ${error.message ?? error}`));
    return;
  }
  clearTimeout(loading);
  if (!current()) return; // the reader moved on, or opened another panel
  if (reading) {
    drawReading(body, ctx, all, found, view, readId, heading);
    revealFocus(body);
    return;
  }
  const results = found.commentaries;
  const sections = results
    .filter((c) => c.notes.length)
    .map((c) =>
      h(
        "section",
        { class: "commentary", "data-commentary": c.id },
        h("h3", { class: "commentary-name" }, c.name, h("span", { class: "commentary-meta" }, ` · ${c.author} · ${c.tradition}`)),
        c.notes.map((n) => note(n, ctx)),
        h("p", { class: "commentary-credit" }, c.credit),
      ),
    );
  // The rest in a line each: silent here, or not on this book at all
  const code = codeOf(ctx, view.book);
  const info = new Map(all.map((c) => [c.id, c]));
  const silent = results.filter((c) => !c.notes.length).map((c) => info.get(c.id));
  const elsewhere = silent.filter((c) => code && !c.books.includes(code));
  const quiet = silent.filter((c) => !elsewhere.includes(c));
  const nothing = [];
  if (quiet.length) {
    nothing.push(h("p", { class: "empty" }, `No note on this verse from ${list(quiet.map(shortName))}.`));
  }
  if (elsewhere.length) {
    nothing.push(h("p", { class: "empty" }, `Nothing on ${ctx.bookName(view.book)} from ${list(elsewhere.map(shortName))}.`));
  }
  // Every commentary follows the KJV's numbering; say so where this translation's differs
  let kjv = null;
  if (results.length && !found.same) {
    kjv = found.kjv
      ? h("p", { class: "notes-kjv" }, `The commentaries number it as the KJV does: ${found.kjv}.`)
      : h("p", { class: "notes-kjv" }, "The KJV, whose numbering the commentaries follow, has nothing that matches this verse.");
  }
  replace(
    body,
    h(
      "p",
      { class: "notes-where" },
      `On ${where}`,
      h("button", { type: "button", class: "text-button notes-whole", onclick: () => ctx.selectVerse(null) }, "Read the whole chapter"),
    ),
    kjv,
    chooser(ctx, all),
    results.length ? [sections, nothing.length ? h("div", { class: "notes-silent" }, nothing) : null] : h("p", { class: "empty" }, "Choose a commentary above."),
  );
  revealFocus(body);
}

/** Open and show the note a search result asked for, where it's in the panel. */
function revealFocus(body) {
  if (focus) {
    focus.at = shown;
    // The note by its label, and which of those with the same label (Tyndale has several
    // introductions to Genesis)
    const same = [...body.querySelectorAll(`[data-commentary="${CSS.escape(focus.id)}"] .note`)].filter((n) => n.dataset.label === focus.label);
    const target = same[focus.nth] ?? same[0];
    if (target) {
      if (target.tagName === "DETAILS") target.open = true;
      target.classList.add("is-focus");
      target.scrollIntoView({ block: "start" });
    }
  }
}
