# Formatting commands — shared specification (1.6)

*md 1.6, issue "Formatting commands and keyboard shortcuts" (proposal rank 10).
Written 2026-10-10 against the TypeScript reference in
`tools/formatting-reference/` (md.win), which also generates the oracle.*

Two pure functions, ported behaviour-for-behaviour to Swift (md and md.macOS),
Kotlin (md.Android) and C# (md.win, `Md.Core`):

```
format(text, selectionStart, selectionEnd, op)      -> Edit | null
wrapTyped(text, selectionStart, selectionEnd, typed) -> Edit | null

Edit = { start, end, text, selectionStart, selectionEnd }
```

`Edit` replaces `[start, end)` of the **original** text with `text`; the
selection is given in the **resulting** text. `null` means no edit at all (the
editor does nothing, and puts nothing on the undo stack).

The oracle is `formatting-vectors.json` — 127 `format` and 10 `wrapTyped`
vectors, copied verbatim into every port's tests, exactly as
`typing-vectors.json` is for SmartTyping. No port may diverge from a vector; a
disputed vector is changed in the reference first and regenerated, in one
commit across the repos. `SmartTyping` is **not** touched by any of this.

## 0. Definitions

- Offsets are **UTF-16 code units**. A port whose string is not UTF-16 (none
  today) converts at its edge.
- A **line break** is U+000A only. CR is an ordinary unit (each port's adapter
  already normalises to LF, as SmartTyping's does).
- **Blank unit**: SP (U+0020), TAB (U+0009) or LF. Only these — never a
  platform "whitespace" predicate.
- A **blank line** is a line of SP and TAB only (or empty).
- A selection is normalised so `selectionStart ≤ selectionEnd`. A **caret** is
  an empty selection.
- **Trimmed selection** `[s′, e′)`: the selection without its leading and
  trailing blank units.
- **Touched lines**: from the line holding `selectionStart` to the line
  holding `selectionEnd`, except that a non-empty selection ending just after
  an LF (at column 0 of the next line) leaves that next line out.
- **Participating lines**: the touched lines, minus blank lines when more than
  one line is touched. (A caret on a blank line participates.)
- `runBefore(p, c)` / `runAfter(p, c)`: the number of consecutive units equal
  to `c` ending at / starting at offset `p`.

## 1. Inline: bold, italic, code, strikethrough

| op | marker | width w | "present" for runs (a, b) | removes |
| --- | --- | --- | --- | --- |
| `bold` | `*` | 2 | a ≥ 2 and b ≥ 2 | 2 |
| `italic` | `*` | 1 | a odd and b odd | 1 |
| `italic` (alternative) | `_` | — | a = 1 and b = 1 | 1 |
| `code` | `` ` `` | 1 | a = 1 and b = 1 | 1 |
| `strikethrough` | `~` | 2 | a ≥ 2 and b ≥ 2 | 2 |

`m` is the marker repeated `w` times. The `_` row is a second marker that
`italic` recognises for removal only; it never inserts `_`.

**Caret at p.** If `runBefore(p, c) = w` and `runAfter(p, c) = w` exactly —
the caret sits in an empty pair of this marker — remove the pair:
edit `[p−w, p+w) → ""`, caret at `p−w`. Otherwise insert `m m`, caret between.

**Selection [s, e).** Try in order; the first that applies wins. Each step is
tried with the main marker first, then (italic only) with `_`.

1. *Inside*: `lead = runAfter(s, c)` bounded by e, `trail = runBefore(e, c)`
   bounded by s. Only when `lead + trail < e − s` (something other than markers
   is inside) and "present"(lead, trail): replace `[s, e)` with the selection
   minus `removes` units at each end; select that inner text.
2. *Outside*: "present"(`runBefore(s, c)`, `runAfter(e, c)`): replace
   `[s − removes, e + removes)` with the selected text; select it.
3. *Wrap*: if the trimmed selection is empty, behave as a caret at `s`
   (insert `m m` at s). Otherwise replace `[s′, e′)` with `m + text + m` and
   select `[s′ + w, e′ + w)`.

So bold on `*x*` makes `***x***`, italic on `***x***` makes `**x**`, and bold
on `***x***` makes `*x*` — the runs compose.

## 2. Link

- Caret, or a trimmed selection that is empty: insert `[]()` at `s`, caret
  after `[`.
- Trimmed selection that starts with `http://` or `https://` (ASCII
  case-insensitive) and holds no blank unit: replace it with `[](URL)`, caret
  after `[`.
- Any other trimmed selection: replace it with `[TEXT](url)` and select the
  placeholder `url`.

## 3. Line operations

Every line operation decides a **change** per touched line — replace
`[at, at+old)` of that line with `new` — or no change. The edit replaces
`[start of the first touched line, end of the last touched line)` (the LF of
the last line is never part of it) with the changed lines joined by LF. If no
line changes, the result is `null`.

**Positions** (both selection ends) are mapped line by line: a position on a
line with no change moves by the earlier lines' deltas; on a changed line, a
position at or after `at + old` moves with the change, one at or before `at`
stays, and one inside the replaced span goes to `at + new.length`. A position
after the last touched line takes every delta. (So a caret at column 0 when a
marker is inserted at column 0 ends after the marker.)

### 3.1 Markers recognised on a line

- **Heading**: up to 3 SP, then 1–6 `#`, then SP or end of line. The marker
  span runs from the first `#` through that one SP.
- **List marker** after the line's leading SP/TAB run: `-`, `*` or `+`, or 1–9
  ASCII digits followed by `.` or `)`; then SP or end of line. The marker span
  includes the one SP.
- **Task**: a bullet or ordered marker followed by `[ ]`, `[x]` or `[X]`, then
  SP or end of line. Its span runs to after that SP.
- **Quote**: up to 3 SP, then `>`, plus one following SP if present.

### 3.2 Headings — `heading1` … `heading6`, `headingCycle`

`heading N`: if every participating line is already a level-N heading, remove
the heading marker from each; otherwise set every participating line to level
N: an existing marker is replaced by N `#` and one SP (no change if it already
is exactly that), a line without one gets N `#` and SP inserted after its
leading spaces when they are at most 3 SP and hold no TAB, else at column 0.

`headingCycle` (the phone bar): the level of the first participating line
decides — none → 1, 1 → 2, 2 → 3, 3 or deeper → none — and that level is set
on every participating line (none = remove the marker).

### 3.3 Bullet list — `bulletList`

If every participating line has a list marker that is a bullet and not a task,
remove each marker span. Otherwise, per participating line: no marker → insert
`- ` after the leading SP/TAB run; a task or an ordered marker → replace its
whole span with `- `; a plain bullet → unchanged.

### 3.4 Task list — `taskList`

If every participating line is a task, remove each task span (marker and box).
Otherwise, per participating line: no marker → insert `- [ ] `; an ordered
marker → replace its span with `- [ ] `; a bullet → replace its span with the
bullet character followed by ` [ ] ` (a bare `-` gains its space); a task →
unchanged.

### 3.5 Quote — `quote`

"All quoted" means every touched line is a quote, blank lines excepted when
more than one line is touched. If all quoted, remove one quote marker (`>` and
one SP) from each line that has one. Otherwise, per touched line: a quote →
unchanged; a blank line in a multi-line selection → insert `>` at column 0;
any other line → insert `> ` at column 0.

### 3.6 Indent and outdent — `indent`, `outdent`

`indent` inserts two SP at column 0 of every participating line (md's list
level is columns ÷ 2). `outdent` removes, from every touched line, one leading
TAB, or else up to two leading SP; `null` when no line has any.

## 4. Typing over a selection — `wrapTyped`

When the selection is non-empty and the typed unit is `*`, `_`, `` ` `` or
`$`, replace the selection with `typed + selection + typed` and select the
original text (shifted by one). No trimming. Anything else — a caret, any other
character — is `null` and the keystroke is ordinary typing. This is on by
default and can be turned off in the app's settings ("Wrap selection when
typing * _ ` $").

## 5. Chords and surfaces

| Command | Windows / Android | Mac / iPad |
| --- | --- | --- |
| Bold | Ctrl+B | ⌘B |
| Italic | Ctrl+I | ⌘I |
| Link | Ctrl+K | ⌘K |
| Code | Ctrl+Shift+C | ⇧⌘C |
| Strikethrough | Ctrl+Shift+X | ⇧⌘X |
| Heading 1–6 | Ctrl+Alt+1 … 6 | ⌥⌘1 … 6 |
| Bullet List | Ctrl+Shift+L | ⇧⌘L |
| Task List | Ctrl+Shift+T | ⇧⌘T |
| Quote | Ctrl+Shift+Q | ⌘' (see note) |
| Indent / Outdent | Ctrl+] / Ctrl+[ | ⌘] / ⌘[ |

Notes. Headings use ⌥⌘ on Apple — an explicit exception to the family's
Windows-Alt → Apple-Control rule, decided 2026-10-10. Quote is ⌘' on Apple,
not ⇧⌘Q, because ⇧⌘Q is the system's Log Out in the Apple menu on macOS, which
wins over any app item; iPad follows the Mac so the two tables stay equal. On
Windows, Ctrl+Alt is AltGr on many European layouts: a heading chord must not
swallow a character AltGr+digit types (e.g. ² on German) — see the Windows
port's notes.

The phone bar (iPhone, and Android at compact width) carries, in order:
Bold, Italic, Code, Link, Heading (`headingCycle`), Bullet List, Task List,
Quote, Outdent, Indent.
