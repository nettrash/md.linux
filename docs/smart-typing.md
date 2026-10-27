# SmartTyping — shared specification (FINAL v1.1)

*v1.1 (2026-09-22): §3.4 rewritten — the adapter tracks md's last capital
through later edits and removing it arms the retype (A.1 row 96); §3.2 macOS
corrected (no `isAutomaticCapitalizationEnabled` on `NSTextView`); §3.3 iOS
observes every character edit through `NSTextStorageDelegate`. The functions,
the vectors and the reference are unchanged.*

Two pure functions, ported behaviour-for-behaviour to Swift (md + md.macOS, one
shared file), Kotlin (md.Android) and C# (md.win, `Md.Core`):

```
enter(text, selectionStart, selectionEnd)            -> EnterEdit | null
capitalize(text, selectionStart, selectionEnd, typed) -> string | null
```

A shared JSON vector file, `typing-vectors.json` (§4), is the cross-port oracle,
exactly as `plot-vectors.json` is for the plotter. No port may diverge from a
vector; a disputed vector is changed in the JSON first, in one commit across all
repos. This document is self-contained: an implementer reads it and the vectors,
nothing else. Where it names a line of `MarkdownParser.swift` that is provenance,
not a dependency — every rule is restated here in full.

The functions follow **md's own parser and renderers**, not CommonMark, wherever
the two differ (§6 lists the differences). The parser is the same code, line for
line, in all three languages, so "what md renders" is one thing.

Normative words: MUST / MUST NOT / iff. Anything marked *(illustrative)* is an
aid to reading and MUST NOT be used in a port (§0.8).

---

## 0. Definitions

Every later rule cites these by name. A port MUST implement each as a private
predicate over UTF-16 code units / Unicode scalars exactly as written and MUST
NOT substitute a platform library call, however similar it looks (§0.8 explains
why: the family has already been bitten by "whitespace" having four answers).

### 0.1 Units and scalars

- **Unit** = one UTF-16 code unit. All offsets in inputs, outputs and vectors are
  unit offsets (`NSString.length`, Kotlin `String.length`, C# `string.Length`).
  Swift MUST work in `utf16` / `NSString` offsets, never `Character` indices.
- **Scalar** = one Unicode scalar decoded from UTF-16. A lone surrogate is a
  scalar of category `Cs` and belongs to none of the classes below.
- **Line terminator** = U+000A, or U+000D U+000A (one terminator), or a lone
  U+000D. Nothing else ends a line: U+2028, U+2029, U+0085, U+000B and U+000C are
  ordinary non-whitespace units.
- Text the functions **insert** always uses U+000A, even in a CRLF or CR document
  (one unit; the platform adapter normalises if it must, §3).

### 0.2 Lines

`lines` = `text` split on line terminators; a terminator belongs to no line. An
empty document has exactly one line, empty. The last line has no terminator.
`lineStart(i)` / `lineEnd(i)` are unit offsets; `lineEnd` excludes the terminator.
The **caret's line** is the line containing the caret offset; if the caret sits
strictly between the U+000D and the U+000A of one terminator, both functions
return `null` (§0.9). No port may materialise a line array (§0.14).

### 0.3 WS19, blank, trim

**WS19** = the 19 scalars `{U+0009, U+0020, U+00A0, U+1680, U+2000–U+200A,
U+200B, U+202F, U+205F, U+3000}` — Foundation's `.whitespaces`, which
`MarkdownParser.kt` enumerates as `isSpaceCharacter`. All are BMP, so a unit test
and a scalar test agree.

- Every "whitespace", "trim", "blank" and "space-only" test in this document
  means WS19 and nothing else. Ports MUST NOT call `.whitespaces`,
  `isWhitespace`, `isBlank`, `trim()`, `char.IsWhiteSpace`, `String.Trim()` or a
  regex `\s`.
- **Blank line** = every unit is WS19 (an empty line is blank).
- **trim(s)** = remove leading and trailing WS19 units.
- **SP** = U+0020 exactly; **TAB** = U+0009 exactly. Where a rule says SP or
  TAB it means those units only, not WS19 (the parser's marker rules are that
  narrow: a NBSP after `-` is not a list item).

### 0.4 Character classes

Classes are Unicode **general categories of the scalar**, read with
Swift `Unicode.Scalar.properties.generalCategory`, Kotlin
`Character.getType(codePoint: Int)` (the `Int` overload), C#
`Rune.GetUnicodeCategory` — never `Character.isLetter`, `Char.isLetter`,
`char.IsLetter`, `isLowercase`, `Character.isLowerCase`, `Rune.IsLower`, which
answer for a grapheme, for a BMP half, or include `Other_Lowercase`.

- **Letter** = category `Lu`, `Ll`, `Lt`, `Lm` or `Lo`.
- **Lowercase letter** = category `Ll` exactly.
- **Uppercase letter** = category `Lu` exactly.
- **Digit** = U+0030–U+0039 only (the parser's `isASCIIDigit`); no other numeric
  scalar is a digit anywhere in this document.
- **Mark** = `Mn`, `Mc`, `Me`. Marks are not letters, not whitespace: `- ́x` is
  a list item whose content starts with a mark, and typing after it is judged by
  the units that are actually there.
- **Symbol** = category `So` or `Sk` (for the emoji lead group, §2.3).

### 0.5 Escapes

Scanning a line left to right, a U+005C `\` **consumes** the following unit
(whatever it is, including another `\` and a surrogate half). A delimiter unit is
**unescaped** iff it was not consumed. So `\|` is an escaped pipe, `\\|` is an
escaped backslash followed by a live pipe — exactly `splitTableRow`'s scan. A
rule that says "contains `|`" without "unescaped" means a raw unit search.

### 0.6 Columns

The **column width** of an indent run: SP counts 1; TAB advances to the next
multiple of 4 (`col += 4 - col % 4`), counting from column 0 at the start of the
run. Indent comparisons ("strictly smaller indent") compare column widths; indent
**copies** copy the original units verbatim.

### 0.7 Case mapping

`upper(s)` for one scalar `s` is the **simple, one-to-one, locale-independent**
uppercase mapping:

- Swift: `s.properties.uppercaseMapping`, accepted only when its
  `unicodeScalars.count == 1` (Swift exposes the *full* mapping; outside the
  Greek ranges excluded below, every scalar whose full mapping differs from
  its simple one maps to 2+ scalars, so the one-scalar guard yields the
  simple mapping).
- Kotlin: `Character.toUpperCase(codePoint: Int)` (the `Int` overload).
- C#: `Rune.ToUpperInvariant`.
- Never `String.uppercased()` on a `Character`, `char.ToUpper`,
  `string.ToUpper()`, `TextInfo`, `Char.uppercaseChar()` on a surrogate half, or
  any culture-aware API.

`upper(s)` is **undefined** — and every rule that needs it returns `null` — when
the mapping is not exactly one scalar, when the result equals `s`, or when `s`
lies in one of these ranges:
- U+10D0–U+10FF, U+2D00–U+2D2F (Georgian: Mkhedruli has an uppercase mapping
  to Mtavruli since Unicode 11 but the script has no sentence-initial
  capitalization);
- U+1F80–U+1FAF, U+1FB3, U+1FC3, U+1FF3 (Greek letters with ypogegrammeni:
  the simple mapping — Java, .NET — is one *titlecase* scalar such as U+1F88,
  the full mapping — Swift, JS — is two scalars; excluding them is the only
  way three ports agree);
- U+00B5 MICRO SIGN (its simple mapping is GREEK CAPITAL MU on every runtime;
  `µs` or `µm` at a line start must not become `Μs`).

The uppercase mapping is used even where a titlecase mapping
exists (`ǆ` → `Ǆ`, not `ǅ`); .NET has no per-rune titlecase and the case is
academic. The platform's Unicode tables are authoritative; vectors use only
scalars whose category and simple mapping are unchanged since Unicode 6.1
(U+00AA `ª` moved from `Ll` to `Lo` in 6.1, and a vector relies on that).

### 0.8 No regular expressions

Normative definitions are unit/scalar walks. `\s`, `$`, `\w`, `\b` and Unicode
classes differ between ICU (`NSRegularExpression`), `java.util.regex` (ASCII `\s`
and `\w` unless `UNICODE_CHARACTER_CLASS`, which throws on ART) and .NET. A
regular expression may appear here only as *(illustrative)* and MUST NOT be
used in a port.

### 0.9 Inputs, outputs and offsets

Inputs: `text` (a string), `selectionStart`, `selectionEnd` (unit offsets, in
either order — Android and macOS report anchor > focus for a backwards
selection), and for `capitalize` a `typed` string.

1. `start = min(selectionStart, selectionEnd)`, `end = max(...)`. If not
   `0 ≤ start ≤ end ≤ text.length` → `null`. If `start` or `end` lies strictly
   between the U+000D and U+000A of a CR LF terminator, or strictly between
   the high and the low half of a surrogate pair → `null` (an edit there would
   split one scalar in two, and Swift's `NSString` bridge repairs a lone
   surrogate differently from Kotlin and C#; UI carets never land there,
   programmatic selections can).
2. `text′ = text[0, start) + text[end, len)`, `caret′ = start`. **All line,
   prefix and sentence reasoning happens on `text′`** with the caret at `caret′`.
   (With an empty selection `text′ == text`.)
3. `capitalize` returns the string that the platform inserts **in place of the
   selection** (i.e. replacing `[start, end)` of the original text), or `null`
   to insert `typed` unchanged.
4. `enter` returns `null` (the platform inserts its own newline over the
   selection) or an `EnterEdit {location, length, replacement, caret}` that
   addresses the **original** `text`: the platform replaces
   `[location, location + length)` with `replacement` and places a collapsed
   caret at `caret` (an offset into the text *after* the edit), as **one**
   undoable step. The mapping from a rule stated in `text′` coordinates:
   - a rule that replaces `text′[a, caret′)` with `r` returns
     `location = a`, `length = (caret′ − a) + (end − start)`, `replacement = r`;
   - a rule that inserts `r` at `caret′` returns `location = start`,
     `length = end − start`, `replacement = r`;
   - the table rule (§1.1) requires the selection to lie within one line.
     Its insert branches insert after a line end `e` of `text′` (`e ≥ caret′`,
     so `e + (end − start)` is the same position in `text`): with an empty
     selection `location = e`, `length = 0`; with a selection
     `location = start`, `length = e + (end − start) − start`,
     `replacement = text[end, e + (end − start)) + r` — the same edit, written
     as one range that also deletes the selection. Its blank-row branch fires
     only when `start == end`: `location = lineStart(c)`,
     `length = lineEnd(c) − lineStart(c)`; so does its column-0 branch, which
     inserts at `lineStart(c)` with `length = 0`.
   `caret` is always `location + (the stated position within replacement)`.
5. An empty document is one empty line with no previous line: `enter` → `null`;
   `capitalize` → §2 rule A applies.

### 0.10 State scan: front matter, fences, comment blocks

Both functions first classify the caret's line as **normal** or **code**. Code
means: inside front matter, inside a fenced code block (opener and closer lines
included), or inside an HTML comment block (start and end lines included). The
classification is one forward pass from the start of `text′` to the caret's
line (§0.14 bounds its cost).

**Front matter** (the parser's `parseFrontMatter`, all guards):
- `trim(lines[0])` is `---` (YAML; closers `---` and `...`) or `+++` (TOML;
  closer `+++`); else there is no front matter.
- `lines[1]` exists and is not blank.
- `close` = the smallest index ≥ 1 with `trim(lines[close])` ∈ closers.
- At least one **field line** exists in `lines[1 ..< close]`: `t = trim(line)`
  is non-empty, does not start with `#`, does not start with `-`, contains the
  separator unit (`:` for YAML, `=` for TOML), and the units before the first
  separator are not all WS19.
- If all hold, lines `0 … close` inclusive are **front matter** (code).

**Tentative front matter** (being typed; the parser sees no block yet): if
`trim(lines[0])` is `---` or `+++`, the test above fails, and **no line in
`lines[1 ..< c]` is a closer line** (a block that is already closed above the
caret is complete and rejected — the parser has decided it is prose: a rule,
a heading, a rule), then the caret's line `c ≥ 1` is code iff every line of
the **tested range** is non-blank and **field-like**:
- the tested range is `lines[1 ..< c]` for `capitalize` (the caret's own line
  is being typed: `---⏎t` never capitalizes a YAML key, and for `c = 1` the
  range is empty and the test holds vacuously) and `lines[1 … c]` for `enter`
  (the line is complete when Enter is pressed: `---⏎- a⏎` continues the list);
- `t = trim(line)` is a **field** iff it does not start with `#` or `-`,
  **contains the separator unit, and has at least one non-WS19 unit before
  it** (`title: x` — exactly `parseFrontMatter`'s guard, which skips `#` and
  `-` lines, so `- key: v` is never a field: `---⏎- key: v⏎---` renders as a
  rule, a list and a rule, and `---⏎- key: v⏎- ` is an empty list item,
  `enter-frontmatter-tentative-list-of-mappings`). `t` is **field-like** iff
  it is a field, or it starts with `#` **and an earlier line of the range is
  a field** (a comment under a key, YAML or TOML), or — for YAML only — it
  starts with `-` and an earlier line of the range is a field (a YAML list
  under a key; TOML has no `-` lists, so `+++⏎key = v⏎- a` is a Markdown
  list, `enter-frontmatter-toml-inside`). A list or a heading directly under
  a bare `---` is Markdown: `---⏎- a⏎- b` is a rule and a list,
  `---⏎tags:⏎- a` is front matter being typed.

YAML/TOML lists inside front matter are deliberately not continued by
`enter`. Accepted cost, pinned by vectors: `---⏎Note: this is prose⏎t` is
still `t` (a prose line with a colon under a rule looks like a field), and
`---⏎# c⏎t` capitalizes although `# c` could be a YAML comment before the
first key.

**Fences and comments**, walking lines after the front matter (or from line 0)
with `state ∈ {normal, fence(char, n), comment}`:
- In `normal`: the line is a **fence opener** iff its leading SP count (TAB does
  not count and stops the count) is ≤ 3, the next unit is `` ` `` or `~`, the
  run of that unit has length ≥ 3, and — for a backtick fence — the rest of the
  line contains no backtick. Enter `fence(char, runLength)`; the opener line is
  code. Else the line is a **comment start** iff, after dropping leading SP
  only, it starts with `<!--`: enter `comment`; the line is code; if this same
  line contains `-->` anywhere, `comment` ends after it.
- In `fence(char, n)`: the line **closes** the fence iff, after dropping *every*
  leading SP (no limit), it has a run of `char` of length ≥ `n` followed only by
  WS19 units. The closer line is code; state returns to `normal` after it. Every
  other line is code. A fence still open at the caret's line makes it code.
- In `comment`: every line is code; the first line containing `-->` ends the
  comment (that line is code).
- The state scan sees only lines at quote depth 0: `> ```` is not a fence
  and `> <!--` is not a comment (§5 records both divergences).

The scan also records, for §2, the index of the **last special line** above the
caret's line: the most recent line that is blank, front matter, a fence line, a
comment line, or an ATX heading (§0.12).

### 0.11 Prefix grammar

Parsed left to right on a line, each part possibly empty:

```
prefix   := indent0 quotes indent1 marker? box?
indent0  := run of SP / TAB
quotes   := ( run of SP, ">", at most one SP )*        -- the parser strips exactly this per level
indent1  := run of SP / TAB                             -- empty unless quotes is non-empty
marker   := ("-" | "*" | "+" | Digit{1,9} ("." | ")")) followed by SP
box      := run of SP, ("[ ]" | "[x]" | "[X]"), followed by SP
```

- **Quote depth** = number of `>` in `quotes`. `indent1` exists only after a
  quote marker; without quotes the list indent is `indent0`. `quotes` is
  recognised only when `indent0` contains no TAB (the parser's `isQuote` drops
  SP only): `\t> x` is a paragraph whose content is `> x`.
- **List indent** = `indent1` when `quotes` is non-empty, else `indent0`,
  measured in columns (§0.6) from column 0 at the start of that run. The
  **list level** = `floor(columns / 2)` — the parser's `indent / 2`
  (`listMarker`), so a 1-SP item under a 0-SP item and a 3-SP item under a
  2-SP item are **siblings**, exactly as rendered. §1.3a compares levels.
- `marker` is absent when the line's inner text (the line minus `indent0` and
  `quotes`, §0.13) is a **thematic break** (§0.12) — tested before the marker,
  as the parser tests a quote's inner lines (`> - - -` has no marker) — or when the digit run is longer than
  9, or when the unit after the marker is not SP (`-\tfoo` and `- foo` are
  paragraphs). A marker at the **line end** (`-`, `1.`, `2020.`) is content
  for both functions although the parser renders it as an empty item (§6):
  Enter must not delete a `2020.` line, and a typed letter would land directly
  after the unit (`-` + `a` is `-a`, not an item) — A.1.
- The **number** of an ordered marker is its digit run read as a decimal
  integer (leading zeros allowed: `01.` is 1).
- `box` is only recognised after `marker`; it is absent when the unit after it
  is not SP (`- [x]\tdone` has content `[x]\tdone`, `- [ ]` has content `[ ]`).
  A box that is the **whole content** of the line (`- [ ]`, `1. [x]`, `- [X]`:
  the three units directly after the marker's SP run, then the line end) is
  content for the caret test (§1.2) and for §1.3 (the item is not empty:
  nothing is deleted), but the line **has a box** for §1.3a and §1.4 — the
  parser renders an empty task, and Enter continues the checklist with
  `- [ ] ` (`enter-empty-task-no-space`, A.1). `- [ ]\t` is not that shape.
- `marker` and `box` each **absorb the run of SP** that follows them (the
  parser drops every SP after both): `-   item` has content `item`, and a
  caret inside that run is inside the prefix (§1.2).
- The **content** of the line is everything after the prefix. The **prefix
  end** is the offset where content starts.

*(illustrative)* `>   - [ ] task` parses as `quotes = "> "`, `indent1 = "  "`,
`marker = "-"`, `box = "[ ]"`, content `task`.

### 0.12 Block tests on one line

- **Thematic break**: remove every SP and TAB; the remaining units number ≥ 3
  and are all `-`, or all `*`, or all `_`. (`-\t-\t-` is a break; `- - x` is
  not.)
- **ATX heading**: drop leading SP only; then 1–6 `#`; then SP or the line
  end. (`#tag` is not a heading; `####### x` is not; `  # h` is.)
- **Page break**: `trim(line)` is `\newpage` or `\pagebreak`.
- **Footnote definition**: `t = trim(line)` starts with `[^`, then a non-empty
  run of units from `A–Z a–z 0–9 - _`, then `]:`. The **footnote prefix** is
  everything up to and including `]:` plus the following run of WS19 units.
- **Quote line**: after dropping leading SP only, the first unit is `>`.
- **Comment start**: after dropping leading SP only, the line starts with `<!--`.

### 0.13 Table grammar (the parser's `parseTable` / `splitTableRow`)

**splitCells(row)**: `r = trim(row)`; if `r` starts with `|` drop that unit; if
`r` ends with `|` drop that unit; scan `r` left to right with an escape flag:
`\` sets the flag; while the flag is set the next unit is appended literally
(the `\` itself is kept unless that unit is `|`) and the flag clears; an
unescaped `|` ends a cell; at the end the last cell is appended. Each cell is
trimmed. The result has ≥ 1 cell.

**isTablePair(header, delimiter)** holds iff:
- `header` contains a `|` unit (raw), has no `marker`, is not an ATX heading,
  not a footnote definition, not a comment start;
- `trim(delimiter)` contains `-`, and `delimiter` is not a line that **has a
  `marker` (§0.11) whose content contains no `|` unit**: `- ` and `- x` are
  list items to a writer, while `- | -` and `- |` are delimiter rows (the
  pipe says table, and the parser tests tables before lists), and `-` (a bare
  marker is content, §0.11), `---`, `--`, `:-`, `-:`, `-|`, `|---|`, `-- | --`
  have no marker and stay delimiter rows (§5 `table-delimiter-list-marker`);
- every cell of `splitCells(delimiter)` is non-empty, consists only of `-` and
  `:`, and contains `-`;
- `splitCells(header).count == splitCells(delimiter).count`.

Both lines are tested on their **inner text** (the line minus `indent0` and
`quotes`) and must have the same quote depth.

**tableContext(c)** for the caret's line `c` with quote depth `d` and inner
text `inner(c)`: let `top` be the smallest index such that every line in
`top … c` is non-blank and has quote depth `d`, and no line in `top … c−1` is
front matter, a fence line or a comment line (state scan). Walk `k` from
`top` upwards: if `k + 1 < lines.count` and `isTablePair(inner(k),
inner(k+1))` (same depth), the pair **opens a table** that runs while the
lines below it contain a `|` unit. If every line in `k+2 … c` contains one,
`k` is the **header** `L`. Otherwise let `j` be the first line in `k+2 … c`
without a pipe: it **ended** that table (the parser's row loop stops there)
and, having no pipe, cannot be a header itself, so the walk resumes at
`j + 1` — **greedy consumption**: a second pair is never looked for inside
the first table's rows (`| a |` / `---` / `| 1 |` / `---` / `| x |`: the
table is lines 0–2, line 3 is a rule and `| x |` a paragraph, exactly as
rendered — `enter-table-second-pair-after-rule`). If `k` opens no table the
walk resumes at `k + 1`. Then:
- the caret's line is a **table row** iff `L` exists and `c ≥ L`;
- `N` = `splitCells(inner(L)).count`;
- `c == L` is the **header row**, `c == L + 1` the **delimiter row**, later
  lines **body rows**.
"Contains a `|` unit" is a raw search, as the parser's row loop: an ATX
heading with a pipe under a table (`# T | x`) is a row to both
(`enter-table-heading-with-pipe-is-row`); a comment line inside the run is
code and stops `top`, where the parser would show it as a row (§5
`table-comment-row`). The **table continues below** row `c` iff line `c+1`
exists, has quote depth `d` and contains a `|` unit (§1.1's blank-row test;
the one lookahead besides the delimiter row, §0.14).
A line that is not a table row by this test but whose inner text, after
`indent1`, starts with `|` is a **pipe line** (a header being typed; §2 treats
it like a row). A pipe-less line under a table (`a | b` / `--|--` / `1 `) is
prose until it contains a `|`; the header of a pipe-less table (`ab. | b` /
`--|--`) is a row by the pair test alone, not by the pipe-line shortcut
(`cap-table-pipeless-header-row-caret`).

### 0.14 Cost contract

Both functions run synchronously on the UI thread on every keystroke.
- The state scan (§0.10) is **one forward pass over units** that inspects, per
  line, at most the first four non-SP units plus the units of fence / comment /
  front-matter lines, allocates nothing per line, and never builds a line array.
  Ports MUST NOT call `MarkdownParser.normalizedLines` (or its Kotlin/C# twins)
  or `split` the text. Three steps look **below** the caret, nothing else
  does: the front-matter test's search for `close`, which runs to the first
  closer line or the end of the document (`---` / `title: x` / `prose` /
  blank / `---` with the caret on the blank line is front matter —
  `cap-frontmatter-closed-below-caret`); `tableContext`'s one-line delimiter
  lookahead (`isTablePair(inner(c), inner(c+1))` when the caret is on a
  header row, plus `lineEnd(c+1)` for §1.1's insert point —
  `enter-table-row-1-header`); and §1.1's "table continues below" test on
  line `c+1` (§0.13). A port that stops every scan at the caret's line fails
  the header-row vectors.
- Every other walk is bounded by the caret's **run** — the lines between the
  nearest blank line above and the caret (tableContext, outdent ancestor,
  display-math paragraph) — or by the caret's line.
- Optional, equivalence-preserving: a port MAY expose the scan as
  `scanState(text, from: offset, state, to: offset) -> State` and let its
  adapter cache `(lineStartOffset, state)` from the previous call, resuming from
  it when every edit since (observed through the adapter's own change
  callbacks) occurred at offsets ≥ `lineStartOffset` and no external
  replacement happened; otherwise the cache is dropped. Vectors test the pure
  functions only.

---

## 1. `enter(text, selectionStart, selectionEnd) -> EnterEdit | null`

Called when the user presses Return / Enter without Shift (§3). Prepare
`text′`, `caret′` per §0.9. Let `c` be the caret's line, `prefix` its prefix
(§0.11), `content` its content, `before` = `text′[lineStart(c), caret′)`,
`after` = `text′[caret′, lineEnd(c))`. Rules are tried **in this order**; the
first that applies decides.

### 1.0 Code state → `null`

If the caret's line is code (§0.10: front matter, tentative front matter, a
fence line or fence body, a comment line or body) → `null`.

### 1.1 Table row

Applies iff the selection lies **within one line** of `text` (no line
terminator in `text[start, end)`; trivially true when `start == end`) and `c`
is a **table row** by `tableContext(c)` on `text′` (§0.13). The caret may be
anywhere in the row. Let `row` = `"\n" + rowPrefix + "|" + ("  |" × N)` where
`rowPrefix` = the caret line's `indent0 + quotes + indent1` copied verbatim
and `N` = the header's cell count (§0.13; a short body row is padded to the
header, as the renderer pads it). For `N = 3` the row is `|  |  |  |`; it
always uses leading and trailing pipes whatever style the table was written
in. "Insert `row` after the line end `e`" means (§0.9): with an empty
selection `location = e`, `length = 0`, `replacement = row`; with a selection
`location = start`, `length = e′ − start`, `replacement = text[end, e′) + row`
where `e′ = e + (end − start)` — one range that deletes the selection and
adds the row. The tail `text[end, e′)` is copied **verbatim**, line
terminators included: in a CRLF document the replacement mixes the copied
`\r\n` with the inserted row's `\n` (`enter-crlf-table-selection-in-header`,
`enter-crlf-selection-in-table-header`; §0.1). Either way `caret = location +
replacement.length − N × 3 + 1`, i.e. after the new row's `"| "` (`14 + 7 − 3
+ 1 = 19` in `enter-selection-in-table-row`; `e + 1 + rowPrefix.length + 2`
when the selection is empty).

- If `c` is the **header row** and `caret′ == lineStart(c)` → `null`: a plain
  newline at column 0 of the header pushes the whole table down intact — the
  only way to insert a line above a table (A.1). Column 0 is the line start,
  before any indent or quote marker: `¦  | a |` under an item detaches the
  table (`enter-table-header-nested-col0`), while `  ¦| a |` and `>¦ | a |`
  are not column 0 and insert a row. Inside a quote the null newline is an
  unquoted blank line, which ends the quote: there is no gesture that inserts
  a quoted line above a quoted table (`enter-table-header-col0-quoted`).
- Else if `c` is the **header row**: insert `row` after the **delimiter row**,
  `e = lineEnd(L + 1)`.
- Else if `start == end`, the table does **not continue below** `c` (§0.13:
  line `c+1` is absent, blank, at another quote depth, or has no `|`) and
  every cell of `splitCells(inner(c))` is empty (a blank row; the delimiter
  row's cells are never empty): **end the table** — replace the whole line
  `[lineStart(c), lineEnd(c))` with `indent0 + quotes` when `quotes` is
  non-empty, else with `""` (a quoted table ends to `> `, as a quoted item
  does in §1.3b; `indent1` and — outside a quote — `indent0` are dropped, as
  in §1.3b: a blank continuation line ends the item anyway,
  `enter-table-in-item-blank-row-exit`); `caret` = the end of the
  replacement, whatever column the caret was in. A blank row **in the
  middle** of a table is an ordinary row (the branches below): ending the
  table there would turn the rows under it into a paragraph of raw pipes
  (`enter-table-blank-row-mid-table`).
- Else if `start == end`, `c` is a **body row** and `caret′ == lineStart(c)`:
  insert the blank row **above** — `location = lineStart(c)`, `length = 0`,
  `replacement = rowPrefix + "|" + ("  |" × N) + "\n"`, `caret = location +
  rowPrefix.length + 2`. The caret's row moves down intact, exactly as a plain
  newline would push it, and the writer gains "insert a row above", which has
  no other gesture (A.1). The delimiter row keeps the branch below: a row
  between header and delimiter would break the table
  (`enter-table-delimiter-caret-at-line-start`).
- Else insert `row` after the caret's row, `e = lineEnd(c)`.

A selection that spans two rows is not this rule: the merged line of `text′`
is judged by §1.2–§1.5 (it has no marker, so the platform splits it — the
honest edit for a selection across rows).

### 1.2 Caret inside the prefix → `null`

If `caret′ < prefix end` → `null` (the platform inserts a plain newline before
the marker; `-| item` becomes `-` / ` item`, `1.| a` becomes `1.` / ` a`). This
includes a caret inside the run of SP a marker absorbs (`- |  ` is `null`, not
an empty item — A.1).

### 1.3 Empty item

Applies iff `content` is blank (WS19 only) and the line has a `marker` or a
non-empty `quotes`. (§1.2 is tested first, so the caret is at or after the
prefix end here.)

**a. Outdent** — applies iff the line has a `marker` and an **ancestor** exists.
Walk up from line `c − 1`; stop (no ancestor) at a blank line, at a line whose
quote depth differs from `c`'s, at a table row, at a line whose **inner text**
(§0.13) is blank (`> ` between quoted items: the renderer sees a blank line
and ends the list), or at a line that is a thematic break, an ATX heading, a
page break (the three tests run on the line's inner text too, so `> ---` and
`> # h` stop the walk inside a quote), or code by the state scan (an indented
fence closer inside an item stops it: `enter-outdent-stops-at-fence-inside-item`).
Lines without a marker (continuation prose, a bare `-` line, `*b*`) are walked
through. The ancestor is the **first** line met whose `marker` exists and
whose **list level** (§0.11, `floor(columns / 2)`) is **strictly smaller**
than `c`'s: `- a` / ` - ` (levels 0, 0) has no ancestor and terminates, as the
renderer shows a sibling; `- a` / `  - b` / `   - ` (0, 1, 1) outdents to
`- ` (`enter-outdent-one-space-child`, `enter-outdent-three-space-sibling`).

Replace `text′[lineStart(c), caret′)` with
`A.indent0 + A.quotes + A.indent1 + marker′ + box′` where `A` is the ancestor
line (indents copied verbatim), `marker′` = the ancestor's bullet unit + SP, or
for an ordered ancestor `next(A.number) + A.delimiter + SP` (`next` per §1.4),
and `box′` = `"[ ] "` iff the ancestor has a `box` (a bare `- [ ]` line has
one, §0.11), else `""`. `caret` = the end of the replacement.

**b. Terminate a list item** — the line has a `marker` and no ancestor.
Replace `text′[lineStart(c), caret′)` with `indent0 + quotes` when `quotes` is
non-empty, else with `""` (a quoted item ends to a quote line `> `; a top-level
item ends to an empty line). `caret` = the end of the replacement.

**c. Terminate a quote level** — the line has no `marker` and non-empty
`quotes`. Drop the **last** quote group (its run of SP, the `>` and its optional
SP): replace `text′[lineStart(c), caret′)` with `indent0 + quotes′`, or with
`""` when `quotes′` is empty. `> > |` → `> `; `> |` → ``. `caret` = the end of
the replacement.

### 1.4 Continue

Applies iff the line has a `marker`, or a non-empty `quotes` (content non-blank,
or the caret at/after the prefix end with content after it).

Insert at `caret′`: `"\n" + indent0 + quotes + indent1 + marker′ + box′` with
- bullet: `marker′` = the same bullet unit + SP;
- ordered: `marker′` = `next(number) + delimiter + SP`, where
  `next(n) = n + 1`, except that when `n + 1` would need more than 9 digits
  `next(n) = n` (the parser rejects longer markers). The number is written in
  decimal without leading zeros (`01.` → `2.`, `999999999.` → `999999999.`).
  There is no "keep 1" rule: every md renderer prints the literal ordinal, so
  `1. 1. 1.` would show as such.
- quote-only line: `marker′ = ""`;
- `box′` = `"[ ] "` iff the line has a `box` (any state; a box that is the
  whole content counts, §0.11: `- [x]` + Enter → `\n- [ ] `), else `""`.
Exactly one SP follows the marker whatever spacing the current line used.
`caret` = after the inserted prefix; `after` moves to the new line (that is
what an insertion at the caret does).

The source numbering of items **below** an item inserted mid-list is not
rewritten (`1. a⏎` before `2. b` yields `1. 2. 2.`); v1 accepts this.

### 1.5 Otherwise → `null`

Headings, thematic breaks, paragraphs, pipe lines that are not table rows, a
bare marker at the line end (`-`, `1.`, `2020.` — content, §0.11), pasted
bullets (`• item`, `— Привет.`: §2.3 leads, not markers — A.1) and the opener
line of a fence all take the platform's plain newline.

### 1.6 Worked examples *(illustrative; the vectors are normative)*

| text′ (`|` = caret)          | result                                   |
| ---------------------------- | ---------------------------------------- |
| `- item|`                    | insert `\n- `                            |
| `- [x] done|`                | insert `\n- [ ] `                        |
| `- [ ]|`                     | insert `\n- [ ] ` (a bare box keeps the checklist going) |
| `3) c|`                      | insert `\n4) `                           |
| `>   - b|`                   | insert `\n>   - `                        |
| `- a` / `  - |`              | outdent → line becomes `- `              |
| `- [ ] a` / `  - [ ] |`      | outdent → `- [ ] `                       |
| `1. p` / `   - |`            | outdent → `2. `                          |
| `- |`                        | terminate → ``                           |
| `-|`, `2020.|`               | `null` (a marker at the line end is content) |
| `\| a \|` / `---` / `|\| 1 \|` / `\| 2 \|` | insert `\|  \|⏎` above row 3, caret in it |
| `> - |`                      | terminate → `> `                         |
| `> > |`                      | terminate → `> `                         |
| `\| a \| b \|` / `\|---\|---\|` / `\| 1 \| 2 \||` | insert `\n|  |  |` after row 3 |
| `\| a \| b \|` / `\|---\|---\|` (caret at end of row 1) | insert after row 2 |
| `\| a \| b \|` / `\|---\|---\|` / `\|  \|  \||` | end table → row 3 becomes `` |
| `# Title|`                   | `null`                                   |
| `-| item`                    | `null` (caret inside the prefix)         |
| `-|item`                     | `null` (no marker: a paragraph)          |
| `> \| a \|` / `> ---` / `> \|  \||` | end table → row 3 becomes `> `    |
| `\| a \|` / `---` / `\|  \||` / `\| 1 \|` | insert `\n|  |` after row 3 (a blank row mid-table is a row) |
| `\| a \|` / `- |`             | terminate → line 2 becomes `` (§5)      |
| `\| a \| b \|` / `- \| -|`     | insert `\n|  |  |` (a marker line with a pipe is a delimiter row) |
| `- a` / ` - |`               | terminate (levels 0 and 0: a sibling)    |

---

## 2. `capitalize(text, selectionStart, selectionEnd, typed) -> string | null`

Called with `typed` = exactly **one scalar** the user is inserting (how the
platform adapters reduce word insertions, composition and dead keys to one
scalar is §3.3; the pure function never sees more). Returns the string to insert
in place of the selection, or `null` to insert `typed` unchanged. Prepare
`text′`, `caret′` per §0.9; `c`, `prefix`, `before`, `after` as in §1.

The function capitalizes **the first letter of every line** and **the first
letter of every sentence within a line**, Markdown-aware. A line is a
paragraph start because md renders every newline inside a paragraph as a
visible line break (`<br>`) — the Notes/Bear model, not CommonMark soft
wrapping — and because every platform keyboard capitalized after Return before
md took the rule over. §5 records the two joins the parser performs (list-item
and footnote continuations) as accepted divergences.

### 2.1 Preconditions — any failing → `null`, tested in this order

a. `typed` is a **Lowercase letter** (`Ll`) and `upper(typed)` is defined
   (§0.7). Anything else — uppercase, digits, marks, Georgian, `ß`, a lone
   surrogate, RTL scripts without case — returns `null` at once.
b. The caret's line is not **code** (§0.10).
c. The caret's line is not a **table row** and not a **pipe line** (§0.13):
   cells hold identifiers and values, so neither rule applies inside tables,
   and a header being typed before its delimiter row exists is treated the
   same way.
d. The caret is not inside **inline code, display math or inline math** by
   the **inline scan** below.
e. Not inside a **link destination**: the last occurrence of the two units
   `](` in `before` has no `)` after it → `null`.
f. The **current token** — the units after the last WS19 unit in `before`, or
   all of `before` when it has none, excluding the units of `prefix2` (§2.3) —
   does not contain `://`, `www.`, `@` or `/`.

There is no HTML-tag precondition: md renders no inline HTML (`MarkdownHTML`
escapes every `<`, the native view shows tags as text), so `<p title="Hi. t`
capitalizes like any prose and `if a<bb. Then` is not penalised (A.1).

**Inline scan.** Let `s` be the index of the last line above `c` that is
special (§0.10: blank, front matter, fence, comment, ATX heading), a
thematic break or a page break (§0.12), or a **table row** (§0.13: the
parser's row loop consumed it, so an unclosed `$$` in a cell stays in the
cell — `cap-display-math-open-in-table-cell-then-prose`) — `s = −1` if none;
these lines end a paragraph and are excluded. Let `k` be the index of the last line in
`s+1 … c` that is a quote line or has a `marker` (§0.11, §0.12) — such a line
starts a new block in the parser's paragraph loop and is **included**; `k =
s+1` if none. `P` = lines `k … c`. Walk `P` line by line, every unit of each earlier
line and only `before` of line `c`, with `display` (paragraph-scoped, starts
`false`) and, reset to `0` / `false` at every line start, `code` (the length of
the backtick run that opened a span) and `inline`. At each unit `u` at index
`i`, with `next` = the unit at `i+1` (on line `c`, when `i+1 == caret′`, `next`
is `typed`; at a line end `next` is absent):

1. if `code > 0`: a backtick run of length exactly `code` closes the span
   (`code = 0`, skip the run); a backtick run of any other length is skipped
   whole (one token, as in CommonMark: `` `` `` never closes a one-backtick
   span); any other unit is skipped;
2. else if `display`: `$$` or `\]` ends it (skip both units); anything else is
   skipped;
3. else if `inline`: a single `$` or `\)` ends it; `\` skips the next unit;
   anything else is skipped;
4. else (`normal`): a backtick run opens a code span of its length; `\[` sets
   `display`; `\(` sets `inline`; any other `\` skips the next unit; `$$` sets
   `display`; a single `$` sets `inline` iff the scalar before it (if any) is
   not a Letter, a Digit, `_` or `$`, **and** `next` exists and is neither WS19
   nor a Digit (`$5`, `$ x`, `a$b` do not open; `$x` does); every other unit
   is skipped.

At the caret: `code > 0 || display || inline` → `null`. Code spans are matched
by run length (CommonMark) and never cross a line; display math crosses lines
within `P`; both are typing-time approximations of the HTML renderer (§5).

### 2.2 Openers, closers, terminators

- **Openers** (may sit between a boundary and the typed letter, any number, any
  order): `*` `_` `~` `"` `'` `(` `[` `{` `«` `»` `‹` `›` `“` `”` `‘` `„` `‚`
  `’` `¿` `¡`, and `!` only when directly followed by `[`. `»` opens a
  German/Danish quotation (`»Hallo.«`), `”` a Swedish/Finnish one
  (`”Hej.”`) and `’` — the apostrophe — opens too when it follows whitespace
  (`’tis`, `’90s`, Swedish `’Hej’`): a `’` inside a word never reaches rule B,
  which needs WS19 before the openers.
- **Closers** (may sit between a terminator and the whitespace): `)` `]` `}`
  `"` `'` `»` `«` `‹` `›` `”` `’` `“` `‘` `*` `_` `~`.
- **Terminators**: U+002E `.`, U+0021 `!`, U+003F `?` only. U+2026 `…` and
  U+0589 (Armenian full stop) are deliberately not terminators in v1; adding
  one is a JSON-first change.

`pre` = `before` with its trailing run of openers removed (the run is maximal;
`!` counts only as the `![` pair). **French spacing:** a guillemet opener
(`«` `‹` `»` `›`) that is preceded by a WS19 unit, by another opener, or by
nothing but `prefix2` (§2.3) also carries the WS19 run after it, so `« `,
`« `, `« ` and `- « ` are opener runs and `« Bonjour »` starts with
a capital (`cap-french-guillemet-space-line-start`); a closing `»` or `«`
after a terminator (`Done.» then`, `»Hallo.« dann`) is preceded by `.` and
keeps its closer role. The **quotation openers** among the openers — `"` `'`
`«` `»` `‹` `›` `“` `”` `‘` `’` `„` `‚` — are remembered for rule B's colon
step (§2.5 1a).

### 2.3 The §2 prefix (`prefix2`)

```
prefix2 := ( prefix WS19* | headingPrefix | footnotePrefix ) lead*
headingPrefix  := indent0 quotes indent1 "#"{1,6} SP WS19*     -- indent0 and indent1 without TAB
footnotePrefix := indent0 quotes indent1 "[^" id "]:" WS19*        -- id per §0.12
lead           := ( section | enum | citation | number | dash | symbols ) WS19+
dash           := run of ( dashUnit | arrow | U+002D ), not a single U+002D
dashUnit       := U+2014 | U+2013 | U+2212 | U+2015 | U+2012 | U+2022 | U+2023 | U+00B7 | U+00A7 | U+00B6
arrow          := U+2190–U+21FF
symbols        := run of ( scalar that is a Symbol (So/Sk), or U+FE0E, U+FE0F, U+200D, U+20E3, or U+1F3FB–U+1F3FF, or keycap )
keycap         := ( Digit | "#" | "*" ) U+FE0F? U+20E3
number         := Digit{1,9} ( "." Digit{1,9} )* ( "." | ")" )?
section        := run of ( U+00A7 | U+00B6 ), WS19*, number
enum           := "("? ( Digit{1,3} | Letter | [ivxIVX]{1,4} ) ")"
citation       := "[" Digit{1,4} "]"
```

`prefix WS19*` means **every** §0.11 prefix — a marker, a box, a quote group,
an indent, or nothing at all — absorbs the WS19 run after it: `- \tt`,
`> t`, `>  t`, `​t` (a zero-width space pasted from the web)
and ` t` all render `t` as the first visible unit of the line, and it
is capitalized (`cap-quote-nbsp-after-marker`, `cap-zwsp-line-start`).
U+FEFF is not WS19 and is content (`cap-bom-line-start`; the loaders strip a
leading BOM, §3.3).

`enum` and `citation` are recognised only as the **first** lead, directly
after `prefix`, `headingPrefix` or `footnotePrefix` (`(1) The parties`,
`a) each`, `(iv)`, `б)`, `- (a) x`, `## (1) x`, `[^3]: (a) The parties`,
`[12] Author, Title` in a reference list; not after a dash lead, not
mid-line: `see (a) then` and `see [12] then` are prose).
`number` alone is recognised only as the first lead after `headingPrefix`
(`## 1.1 Title`, `## 1) x`, `## 2 x`): at paragraph or item level a
number-then-space is a quantity far more often than a section number in md's
own documents (`2.5 cups`, `- 3.5 kg`), so `1.1 Scope` stays lowercase (A.2).
`section` (`§ 3 `, `§3 `, `§ 12.1 `) may follow other leads; a bare `§ ` or
`¶ ` is a dash lead (U+00A7 and U+00B6 are `Po` since Unicode 6.1, not `So`,
but a lead to a writer).

`prefix2` is measured on **`before`** (the line up to the caret), not on the
whole line: a caret inside the SP run a marker absorbs (`- | item`,
`#  |Title`, `>| x`) is still at the content start, because the typed letter
becomes the first content unit of what md renders. `prefix` is §0.11's
(indent, quotes, marker, box — a marker or box at the line end is content
there too: `-` + `a` is `-a`, `1.` + `a` is `1.a`, `- [ ]` + `x` is
`- [ ]x`). `headingPrefix` absorbs the WS19 run after its SP, as the parser
trims the heading text, and counts only when `indent0` and `indent1` contain
no TAB (the parser's `parseHeading` drops SP only: `\t# h` is a paragraph,
§6). `lead` groups let a Russian dialogue line (`— куда`, `― куда` with the
U+2015 quotation dash of typeset books, `-- куда` on a keyboard without an
em dash), a pasted bullet (`• item`, `◦ item`: what Notes, Pages and Word
produce), an arrow bullet (`→ next`, `⇒ so`: U+2190–U+21FF, at the content
start only — `a → b. c` is untouched, §2.5) and a status line (`✅ done`,
`- 🔥 hot`, `1️⃣ first`) start with a capital. A **single** hyphen-minus is
never a lead: `- ` is a list marker, and `— - x` stays lowercase. A `#`
without a following SP, or seven `#`, is content, not a heading prefix.

### 2.4 Rule A — line start

If `pre` is exactly `prefix2` measured on `before` (the typed letter is the
first content unit of its line, openers aside), **or `pre` is empty** (the caret is at column 0
ahead of whatever prefix the line has — `|- item`, `|  text`, or a selection
that starts at the line start is being replaced — so the typed letter becomes
the line's first unit) → return `upper(typed)`. This holds whatever the
previous line is: prose, a quote line, a heading, a rule, a blank line, or
nothing (the empty document).

**Exception — a task box being typed by hand:** when the line has a
`marker` and no `box`, `before` is exactly `prefix` + `[` (the `[` directly
after the marker's SP run) and `typed` is `x` (U+0078) → `null`, before
rules A and B: `- [x] done` typed by hand must not become `- [X] done`
(`cap-task-box-typed-x`). Any other letter after that `[` is link text and
is capitalized (`- [Home](…)`, `cap-link-text-at-item-start`).

### 2.5 Rule B — sentence start within the line

Scan `pre` backwards from its end:

1. ≥ 1 WS19 units (a line break is never crossed — rule A owns line starts);
   1a. **direct speech after a colon**: if the unit before that WS19 run is
   `:` and at least one quotation opener (§2.2) was removed from `before` to
   form `pre` → return `upper(typed)` at once, no token tests (`Он сказал:
   «Привет»`, `He said: "Hello."`, `Er sagte: „Hallo“` — Розенталь §49 and
   every English style guide capitalize here; `note: the` and `He said: (`
   are untouched, `cap-colon-guillemet-ru`, `cap-colon-no-quote`);
   1b. then, optionally, **one group** preceded by ≥ 1 WS19 units: either a
   **dash group** — a run of units each of which is a `dashUnit` (§2.3) or
   U+002D, mixed freely (`—`, `- `, `--`, `—-`: keyboards without an em dash
   type the dialogue dash as a hyphen; arrows are not in this group) — or a
   **symbols group**, a `symbols` run per §2.3, keycaps included (`Done. 🎉
   next`, `Готово. ✅ дальше`: the status notes the leads were added for,
   mid-line). When a dash group is present the terminator run of step 3 must
   end in `.` (its last unit, the one nearest the closers): `сказал он. —
   Пойду` resumes the speech with a capital, `Куда? — спросил` and `Стой! —
   сказал` are dialogue tags and stay lowercase; a symbols group has no such
   restriction (`Wait? 🤔 next`);
2. then zero or more closers, where a **footnote reference** `[^id]` (id per
   §0.12) or a **citation** `[` Digit+ `]` counts as one closer (Chicago
   style puts the reference after the period: `text.[^1] Next`, `text.[12]
   Next`; `[x]` is a plain closer and a letter); when at least one closer was
   consumed, then zero or more WS19 units (French spacing inside guillemets:
   `Ça va ? »`; also `text. [1] Next`);
3. then the **terminator run**: the maximal run of units from `. ! ?`, length
   ≥ 1. If the run contains **two or more `.`** it is an ellipsis (`...`,
   `?..`, `!..`) → `null`. A run containing `?` or `!` **ends the sentence
   whatever the token**: tests (ii) and (iii) of step 4 apply only to a run
   that is the single `.` (`what is x? I`, `find x! Then`, `etc.? Next`,
   `и т.д.? Нет` capitalize; abbreviations and initials never end in `?`);
4. then the **token**: the maximal run of non-WS19 units ending just before the
   terminator run (possibly empty), with its leading openers removed. If the
   token is empty, the run contains no `.`, exactly **one** WS19 unit precedes
   the run, and a non-empty run of non-WS19 units precedes that unit without
   reaching into `prefix2`, that run is the token instead (French spacing
   before `!` and `?`: `Bonjour !`, `Ça va ?`, with SP, U+00A0 or U+202F;
   `?! ` and `. ` at a line start stay empty). Return `null` if the token is
   - (i) empty;
   - (ii) exactly one Letter scalar followed by zero or more Marks (so the NFC
     and NFD spellings of `é.` agree) and either
     - the letter is not an Uppercase letter — a numbered unit (`с. 15`,
       `p. 42`, `v. 2`) or the first half of a spaced abbreviation (`т. е.`,
       `z. B.`, `p. ex.`) — **except** U+044F `я`, the one single-letter word
       that ends a Russian sentence; or
     - the letter is an Uppercase letter **and the previous token** — the
       run of non-WS19 units before the ≥ 1 WS19 units that precede the
       token, with leading openers removed — is itself one Letter (+ Marks)
       followed by a terminator run: an initials chain (`J. R. R.`,
       `А. С. Пушкин`) or a spaced abbreviation (`z. B.`, `o. Ä.`, `u. U.`).
       A single capital after anything else ends a sentence (`Plan B. Then`,
       `vitamin C. It`, `It was I. Then`, `J. Rowling` — the surname is
       capitalized anyway); accepted misses, pinned: `Иванов И. и` and
       `Mr. T. and` get a capital;
   - (ii′) a **compact initials chain**: two or more groups of one
     Uppercase letter (+ Marks) separated by single `.` units — `И.И`,
     `А.С`, `J.R.R`, `U.S.A` (the final `.` is the terminator run) — the
     GOST spelling `Иванов И.И. и др.` and `J.R.R. Tolkien`; `U.S.A. Then`
     is an accepted miss, consistent with `U.S.` in §2.6, as are `A.I. Then`
     and `E.T. Then`. Lowercase groups (`n.b`) and mixed case (`A.b`) are
     not a chain (`cap-russian-compact-initials-then-lowercase`);
   - (iii) in the abbreviation list (§2.6) after folding; a closer after the
     abbreviation (`(etc.) then`, `"Mr." by`) does not change this (A.2).
   Numeric tokens (`3.14`, `42`), tokens containing `/`, `@` or `://`
   (`See https://x.y/docs. Then`), a code span, a footnote reference
   (`text[^1].`) and every other token are ordinary: return `upper(typed)`.

Otherwise → `null`. Mid-word letters, contractions (`don't`) and hyphenations
(`re-enter`) are never capitalized because step 1 requires whitespace
immediately before the openers. A terminator inside link text
(`[Done.](x) then`) is not a sentence end: a link is one token (A.1).

Dialogue tags after `?`/`!` inside quotes (`"Where?" she asked`) are
capitalized like every keyboard and word processor does; fiction writers have
the override (§3.4).

### 2.6 Abbreviations

**Fold**: map U+0041–U+005A to U+0061–U+007A (ASCII A–Z), U+00C0–U+00DE
except U+00D7 to U+00E0–U+00FE (Latin-1 À–Þ, one-to-one: `o.Ä.`, the Duden
spelling, folds to `o.ä`; `TÉL`, `Éd` match), U+0410–U+042F to U+0430–U+044F
(Cyrillic А–Я), U+0401 → U+0451 (Ё), U+0404 → U+0454 (Є), U+0406 → U+0456 (І),
U+0407 → U+0457 (Ї), U+0490 → U+0491 (Ґ); every other unit is left as is —
capitals outside these ranges (`Ğ`) are not folded. Compare the folded token
to the entries below with **ordinal** equality (C#:
`StringComparison.Ordinal`; never default `Equals`, `IndexOf`, `StartsWith`);
entries are stored exactly as listed, final dot removed, inner dots kept.

The list is final for v1; changes are JSON-first (a vector per added entry).

English: `a.d` `a.m` `al` `approx` `apr` `assn` `aug` `ave` `b.c` `blvd` `ca`
`cf` `ch` `co` `corp` `dec` `dept` `dr` `e.g` `e.u` `ed` `eds` `eq` `eqs` `esp`
`etc` `excl` `ext` `feb` `ff` `fig` `figs` `fri` `govt` `i.e` `ibid` `inc`
`incl` `jan` `jr` `jul` `jun` `ltd` `misc` `mr` `mrs` `ms` `mt` `nov` `oct`
`p.m` `ph.d` `pp` `prof` `rd` `resp` `sep` `sept` `sr` `st` `tel` `thu` `tue`
`u.k` `u.s` `univ` `viz` `vol` `vs`

German: `abs` `bspw` `bzgl` `bzw` `d.h` `evtl` `exkl` `geb` `ggf` `hrsg` `inkl`
`jh` `mio` `mrd` `nr` `o.ä` `o.g` `s.o` `s.u` `sog` `std` `str` `tsd` `u.a`
`u.u` `usw` `vgl` `z.b` `z.t` `zzgl`

French: `art` `av` `chap` `éd` `env` `ex` `mlle` `mme` `p.ex` `réf` `ste` `tél`
`trad`

Spanish: `aprox` `avda` `cap` `dña` `dpto` `ej` `núm` `p.ej` `pág` `págs` `sra`
`srta` `ud` `uds`

Russian: `акад` `англ` `г` `гг` `гл` `гос` `греч` `др` `зам` `изд` `им` `исп`
`ит` `кв` `коп` `корп` `лат` `млн` `млрд` `напр` `нем` `н.э` `обл` `пер`
`перев` `пл` `пп` `пр` `прим` `просп` `проф` `ред` `руб` `рус` `св` `см` `сокр`
`сост` `ст` `стр` `табл` `тел` `т.д` `т.е` `т.к` `т.н` `т.о` `т.п` `т.ч` `тыс`
`укр` `ул` `фр` `чел` `чл` `шт` `экз`

Ukrainian: `буд` `вул` `грн` `див` `ін` `рр` `стор` `тис` `т.зв`

List-ending abbreviations (`etc`, `usw`, `u.a`, `т.д`, `т.п`), unit-style
Russian ones (`г`, `гг`, `руб`, `тыс`, `см`) and the times `a.m` / `p.m`
(`from 5 p.m. to 6 p.m.` is as common as `at 5 p.m. Then`) stay in the list
although they end a sentence at least as often as they continue one: a
missing capital costs one Shift, a wrong one the override gesture — decided
once (A.1, `cap-abbr-pm-sentence`). Roman and lettered enumerators written
with a period are an accepted inconsistency: `i. ` and `a. ` are single
letters (B ii, `null`), `ii. ` and `iv. ` are ordinary tokens (a capital) —
pinned rather than special-cased, because a `Letter "."` lead at a line
start would capitalize `т. е.` and `z. B.` (`cap-roman-one-then-period`).

Deliberately absent (also ordinary words, or too short to be safe): `no` `may`
`sat` `sun` `mon` `wed` `mar` `min` `max` `sec` `hr` `ok` `est` `para` `op`
`cit` `рис` `мин` `сек` `ок` `ср` `макс`. Single-letter abbreviations need no
entry — rule B(ii) covers them in every script.

### 2.7 Worked examples *(illustrative; the vectors are normative)*

| text′ (`|` = caret) + typed | result | why |
| --- | --- | --- |
| `` + `h` | `H` | A, empty document |
| `Notes` / `|` + `j` | `J` | A, line start |
| `> The quick` / `> |` + `b` | `B` | A, quote line start |
| `- |` + `i` | `I` | A, list item |
| `>   - |` + `b` | `B` | A, nested item in quote |
| `# |` + `t` | `T` | A, heading |
| `[^1]: |` + `t` | `T` | A, footnote definition |
| `— |` + `к` | `К` | A, dialogue dash |
| `✅ |` + `d` | `D` | A, symbol lead |
| `Hello. |` + `w` | `W` | B |
| `Really?! |` + `y` | `Y` | B |
| `wait... |` + `w` | `null` | ellipsis |
| `Что?.. |` + `о` | `null` | ellipsis |
| `e.g. |` + `n` | `null` | abbreviation |
| `т. |` + `е` | `null` | single letter |
| `Это был я. |` + `о` | `О` | я exception |
| `costs $5. |` + `t` | `T` | `$5` is not math |
| `$x + |` + `t` | `null` | inline math |
| `$$E=mc^2$$ and so. |` + `n` | `N` | display math closed |
| `` ``a`b`` next. |`` + `t` | `T` | run-length span closed |
| `<div class="x |` + `n` | `null` | no terminator (there is no HTML rule) |
| `a < bb. |` + `n` | `N` | `a < b. |` is `null`: `b` is a single lowercase letter, B(ii) |
| `Plan B. |` + `t` | `T` | a single capital after a word ends a sentence |
| `J. R. |` + `t` | `null` | initials chain |
| `Bonjour ! |` + `c` | `C` | French spacing |
| `сказал он. — |` + `п` | `П` | dash group after a period |
| `сказал он. - |` + `п` | `П` | hyphen as the dialogue dash, mid-line only |
| `— Куда? — |` + `с` | `null` | dialogue tag |
| `Done. 🎉 |` + `n` | `N` | symbols group between sentences |
| `what is x? |` + `i` | `I` | a `?` run ends the sentence whatever the token |
| `text.[^1] |` + `n` | `N` | footnote reference after the period |
| `(1) |` + `t` | `T` | enumerator lead |
| `## 1.1 |` + `i` | `I` | number lead in a heading |
| `1.1 |` + `s` | `null` | no number lead at paragraph level (`2.5 cups`) |
| `§ 3 |` + `a` | `A` | section lead |
| `o.Ä. |` + `d` | `null` | Latin-1 fold |
| `• |` + `t` | `T` | pasted bullet lead |
| `→ |` + `t` | `T` | arrow lead (content start only) |
| `-- |` + `п` | `П` | two hyphens are a dialogue dash; one is a marker |
| `« |` + `b` | `B` | French spacing after an opening guillemet |
| `Он сказал: «|` + `п` | `П` | direct speech after a colon |
| `Иванов А.С. |` + `и` | `null` | compact initials chain |
| `[12] |` + `t` | `T` | citation lead (reference list) |
| `- [|` + `x` | `null` | a task box being typed |
| `> |` + `t` | `T` | WS19 after every prefix is absorbed |
| `\t# |` + `t` | `null` | not a heading in md |
| `see https://a.b/c. |` + `n` | `N` | URL token complete |
| `https://a.b/|` + `n` | `null` | inside URL |
| `\| name \|` / `\|---\|` / `\| |` + `n` | `null` | table row |
| `first line` / `|` + `s` | `S` | A: every line start |
| `don'|` + `t` | `null` | no whitespace |
| `---` / `|` + `t` | `null` | tentative front matter |
| `    |` + `t` | `T` | no indented code in md |

---

## 3. Platform contract (not covered by the vectors)

The pure functions are adapters' inputs; everything here is per-platform glue
and is verified on a device, not by the JSON.

### 3.1 Settings

Keys `md.continueLists` and `md.capitalizeSentences`, both bool, default
`true`, on every platform. A function whose setting is off is never called.
Homes: iOS/macOS `@AppStorage` (next to `md.bookViewMode`); Android the
`SharedPreferences` file the app already uses for `md.pdfPageSize` and the
per-file view-mode memory (no second store); Windows two new constants in
`Md.App.Logic/Settings/SettingsKeys.cs`, appended to `All` (the count test and
`shell-final.md` §9 / PRIVACY.md move from eight keys to ten). Windows shows
both in Settings with a one-line hint that Ctrl+Z undoes a capital, because no
Windows Markdown editor capitalizes hardware-keyboard typing today.

### 3.2 Who owns capitalization

md owns it. The keyboard's own sentence capitalization is turned **off** on
every platform so that code fences stop being capitalized and list items,
quotes and headings start being capitalized the same way everywhere:

- iOS: `autocapitalizationType = .none`; `autocorrectionType` stays `.default`
  (it repairs `Ios` → `iOS` after md's capital). Accepted cost, decided here so
  it is not re-litigated: the software keyboard's Shift key no longer
  auto-engages and the predictive bar shows lowercase suggestions at a sentence
  start; md capitalizes what is inserted (§3.3).
- macOS: the four existing `isAutomatic…Enabled = false` lines in
  `MarkdownEditor.swift` (quote and dash substitution, text replacement,
  spelling correction) already keep the system's capitalization out; nothing
  is added. `NSTextView` has no `isAutomaticCapitalizationEnabled` — the only
  property of that name is `NSSpellChecker`'s read-only class property, which
  reports the user's preference and controls nothing here.
- Android: `KeyboardCapitalization.None` — **but only in the same commit that
  lands the verified `InputTransformation` (§3.3)**; until the device checklist
  passes, `Sentences` stays on, because removing it before md's rule works
  trades a visible feature for nothing.
- Windows: verify on a touch device that the touch keyboard does not auto-shift
  in the editor `TextBox` with `IsTextPredictionEnabled = false`; if it does,
  try an `InputScope` without sentence capitalization and document the
  finding in the PR.

### 3.3 Hooks, per platform

**Reduction to one scalar.** An insertion is a **word insertion** iff its text
contains no line terminator and no WS19 unit except at most one trailing SP,
its first scalar is a Lowercase letter, **and the text contains none of
`://`, `www.`, `@`, `/`** (precondition §2.1f applied to the insertion itself:
a pasted or predicted `https://a.b`, `~/Documents/x` or `@nettrash` is
inserted unchanged — the pure function sees only the first scalar and cannot
tell). For a word insertion the adapter
calls `capitalize` with `typed` = that first scalar and the insertion's target
range as the selection; on a non-null result it inserts the word with its first
scalar replaced. Multi-word insertions (dictation phrases, paste) are never
capitalized; iOS additionally overrides `paste(_:)` to set a flag that
**skips** capitalization for the single-word paste case, and treats
`textInputMode?.primaryLanguage == "dictation"` as a skip.

**iOS** — `SmartTextView: UITextView` overrides `insertText(_:)`:
- `markedTextRange != nil` → `super` (CJK composition is committed later as a
  non-`Ll` scalar; Latin marked text on iOS is rare and arrives as a word on
  commit, which the word rule handles).
- `text == "\n"`: `e = enter(self.text, selectedRange)`; if non-null,
  `replace(textRange(e.location, e.length), withText: e.replacement)` then
  `selectedRange = NSRange(e.caret, 0)`; else `super`. Dictation's spoken "new
  line" arrives here too and is treated the same.
- otherwise the word rule; `super.insertText(result ?? text)`.
- `shouldChangeTextIn` is **not** used for these features (it also fires for
  autocorrect replacements and `replace(_:withText:)`).
- The view is also its `textStorage`'s `NSTextStorageDelegate` and reads
  **every** character edit off
  `textStorage(_:didProcessEditing:range:changeInLength:)` (the
  `.editedCharacters` mask): Cut, Paste, a drop, a hardware forward delete,
  Scribble, an autocorrect replacement or revert and UIKit's own undo of a
  typing run never reach `insertText(_:)` or `deleteBackward()` (Paste does
  not even land synchronously), and the storage is the one place every edit
  passes through. That is where the §3.4 tracking follows the capital's
  offset and the armed offset, so a cut before the capital, a paste or a
  forward delete keep both right. md's own edits — the one-scalar capital
  swap, its undo and redo, the `enter` edit — are made under a re-entrancy
  flag and skipped there, because `insertText(_:)` has already made their
  transition; the typed text itself is observed as well, harmlessly, because
  the transition is idempotent. This is how md's iOS port does it.

**macOS** — `SmartTextView: NSTextView` overrides:
- `insertNewline(_:)`: if `NSApp.currentEvent?.modifierFlags.contains(.shift)
  == true` → `super`; else the `enter` edit via
  `insertText(replacement, replacementRange:)` (one undoable typing step), else
  `super`. Shift-Return has no binding of its own in
  `StandardKeyBinding.dict` and falls through to `insertNewline:`.
- `insertLineBreak(_:)` (Ctrl-Return): always
  `insertText("\n", replacementRange: selectedRange())`, never U+2028.
- `insertText(_ string: Any, replacementRange:)`: `string` may be `String` or
  `NSAttributedString`; if `hasMarkedText()` → `super`; else the word rule.
  This is also where a **dead-key** session ends (`ê`, `á`, `ö`: one scalar
  inserted at the range where the session began) and where the press-and-hold
  **accent popover** replaces one scalar by one scalar (`E` → `é`): both are
  one-scalar insertions over a range and go through `capitalize` with that
  range as the selection, so `école` at a sentence start becomes `École`.
- The override must not call the delegate's `shouldChange…` itself; nothing
  touches `textStorage` directly.

**Android** — `BasicTextField(state: TextFieldState, lineLimits = MultiLine,
inputTransformation = SmartTyping)`; the `String` overload in
`EditorScreen.kt` and `DocumentViewModel.onTextChange(String)` are retired.
The transformation sees every user edit (soft keyboard, hardware keyboard,
composition updates) as a `TextFieldBuffer` change list:
- **Enter**: the change set is exactly one insertion of `"\n"` at a collapsed
  original selection → apply `enter(originalText, start, end)` by replacing the
  buffer range and placing the selection at `caret`. A hardware Shift+Enter is
  detected in `onPreviewKeyEvent` (`Key.Enter` with `isShiftPressed`) and
  inserts `"\n"` unconditionally. Soft-keyboard Enter never arrives as a
  `KeyEvent` (it is `commitText("\n")`), which is why the transformation, not
  a key handler, is the hook.
- **Letter**: the change set is exactly one replacement whose inserted text is
  a word insertion → replace its first scalar per `capitalize`. **Composition
  is not a reason to skip**: Gboard sends `setComposingText("h")`, then
  `setComposingText("he")` over the same region; the per-first-scalar rule is
  idempotent under that re-send (`H` stays `He`). `TextFieldValue.composition`
  is not used (rewriting composed text through it restarts IMEs).
- Device checklist for the landing PR: Gboard, Samsung Keyboard, a hardware
  keyboard, swipe input, voice input; each of: list continuation, empty-item
  exit, capital at line start, capital after `. `, no capital in a fence,
  override gesture, undo.

**Windows** — inputs are `_box.Text` (CR-terminated, exactly as the control
reports it; §0.1 already treats a lone `\r` as a line end, so nothing is
converted on the way in) and `_box.SelectionStart` / `SelectionLength`.
- **Enter**: `KeyDown` with `VirtualKey.Enter`, Shift up, no composition in
  progress: `e.Handled = true; _box.Select(location, length);
  _box.SelectedText = replacement; _box.Select(caret, 0)` — the same
  undo-preserving path the Tab key uses in `EditorPane.cs`. `Text =` is never
  assigned for these edits (it clears the undo history).
- **Letter**: `BeforeTextChanging`: diff `e.NewText` against `_box.Text`; if
  the diff is one replacement of the selection by a word insertion →
  `e.Cancel = true` and, guarded by a re-entrancy flag,
  `_box.SelectedText = result; _box.Select(start + result.Length, 0)`. If
  `SelectedText` may not be assigned synchronously inside the handler, defer
  via `DispatcherQueue.TryEnqueue` and re-check that the selection is unchanged
  before applying. `CharacterReceived` is not used (a non-BMP letter arrives
  as two events).
- Composition: skip both features between `TextCompositionStarted` and
  `TextCompositionEnded`.

**Editor text and the BOM.** The editor text never begins with U+FEFF: every
loader strips a BOM on decode (`MarkdownDocument.swift`, `TextCodec.kt`,
`PlainTextCodec.cs`) and writes it back on save where it was. The functions
treat U+FEFF as content (it is not WS19), so a BOM pasted mid-document makes
that line's first unit invisible content: no capital, no list
(`cap-bom-before-marker`, `enter-bom-before-marker-not-list`); a loader that
stopped stripping it would kill both features on line 0.

### 3.4 Override gesture

The adapter tracks the **last capital md produced**: `capital = p`, the unit
offset of that one scalar, until it is removed. There is one tracked capital
at a time. Every edit to the text updates the tracking:

- an edit whose range lies before `p` (its end ≤ `p`; an insertion at `p`
  included) shifts `p` by the edit's length delta;
- an edit that **removes** the capital scalar — a deletion or a replacement
  whose range covers `p`, unless the inserted text puts that same capital
  scalar back at the same offset — sets `capital = nil` and **arms** the
  override at `q` = the edit's start offset;
- an edit after `p` leaves it alone;
- producing a new capital replaces the tracked one (the old one is forgotten,
  not armed).

While armed at `q`:

- a word insertion (§3.3) that starts exactly at `q` is inserted **as typed**
  — no capitalization — and the override is then cleared (Android excepted,
  below);
- any insertion that starts elsewhere than `q` clears the override;
- deletions never clear it: a deletion before `q` shifts `q`; a deletion
  covering `q` moves `q` to its start;
- an external replacement (open, revert, reload, article switch) clears both
  the tracked capital and the override.

Undo that restores the lowercase letter is an edit that removed the capital:
it arms the override at `p`, and the restored letter is not re-judged (§3.5;
on Windows the Undo/Redo paths already bypass the hooks). Independently of
the tracking, a single lowercase letter typed over a one-scalar selection
whose scalar is that letter's `upper` is inserted as typed — and, since it
removed the capital, it arms the override.

**Android exception.** While armed at `q` the override stays armed as long as
the IME keeps replacing a composing region that starts at `q` (each re-send is
inserted as typed) and clears when an insertion starts elsewhere. A
composition re-send whose range starts at the tracked capital `p` and whose
inserted text begins with the lowercase form of that capital is an IME
re-send, not a removal: the capital is re-applied (§3.3's idempotent rule)
and `p` stays; only a change that shrinks or empties the region removes the
capital.

**Consequences.** Every port pins each of these with a test — a hosted view
test where the platform allows it, otherwise the pure state machine plus the
adapter plan:

1. type `md` → `Md`; ⌫ ⌫; type `md` → `md`.
2. type `iOS` → `IOS`; ⌫ ⌫ ⌫; type `iOS` → `iOS`.
3. type `md is` → `Md is` (five keystrokes; the capital is the first, and the
   four insertions after it disarm nothing because nothing is armed); five ⌫;
   type `md is` → `md is`.
4. select the whole word `Md` and type `md` → `md`.
5. after `Md`, place the caret before the `M` and type `a` → `AM` (the
   capital was not deleted: the insertion at `p` shifts it to `p + 1`, and
   the new `A` becomes the tracked capital).
6. type `m` → `M`; Undo → `m` (Apple and Windows restore the lowercase and
   arm the override at `0`); type `d` → `md` (an insertion elsewhere than
   `q` clears the override; nothing re-judges the `m`). Android's Undo
   removes the letter instead — its recorded difference (§3.5) — which arms
   the override at `0`, and the next letter typed there is inserted as typed.
7. type `Xxx. ` then `m` → `Xxx. M`; select and cut `Xxx. ` (before the
   capital); ⌫ deletes the `M`; type `m` → `m` (offset tracking survives the
   cut).
8. type `m` → `M`, type `d` → `Md`, ⌫ (deletes the `d`), type `d` → `Md`
   (the capital still stands; nothing is armed).

This covers the gestures writers actually use — backspace-and-retype,
select-and-retype, undo-and-continue — and, because the tracking is by
offset, they survive whatever else was edited around the capital. Brand
names (`md`, `iOS`, `npm`) at a sentence start rely on it on the three
platforms without autocorrect. The README and CHANGELOG of every port say it
in one plain sentence: "delete the capital md wrote and type the letter
again; it stays lowercase" — no key names in the Apple docs, which name none.

### 3.5 Undo

The capitalization is **its own undo step**: Undo immediately after a capital
restores the lowercase letter, keeps the caret, and arms the override at `p`.
Apple: `breakUndoCoalescing()` before and after the one-scalar replacement so
it does not merge into the typing run. Windows: two `SelectedText` assignments
(the letter, then the replacement) are two undo units. Android: an
`InputTransformation` edit is atomic with the keystroke, so Undo there removes
the letter and the override is the recovery path — recorded as the one
platform difference. Every `enter` edit is one undo step everywhere.

### 3.6 Enter with Shift

macOS, Windows and an iPad hardware keyboard (where `UIKeyCommand` /
`pressesBegan` can see the modifier): Shift-Enter bypasses `enter` and inserts
a plain `"\n"`.

---

## 4. `typing-vectors.json`

One file, copied verbatim into every repo at the path its `plot-vectors.json`
sits (`md/mdTests/PlotVectors/`, `md.macOS/mdTests/PlotVectors/`,
`md.Android/md/src/test/resources/`, `md.win/tests/Md.Core.Tests/Fixtures/`).
Ports decode it with the loader they already use for `plot-vectors.json`
(`JSONDecoder` + `Decodable`, the hand-rolled Kotlin reader, `JsonDocument`)
and assert **every** vector.

### 4.1 Schema

```json
{
  "_meta": {
    "purpose": "Authoritative cross-platform test vectors for md SmartTyping (SPEC.md v1).",
    "generatedBy": "<harness name and commit>",
    "unicodeVersion": "vectors use only scalars stable since Unicode 6.1",
    "offsets": "UTF-16 code units",
    "counts": { "enter": 0, "capitalize": 0 }
  },
  "enter": [
    { "id": "kebab-case-unique", "note": "optional prose",
      "text": "…", "start": 0, "end": 0,
      "expected": null }
  ],
  "capitalize": [
    { "id": "kebab-case-unique", "note": "optional prose",
      "text": "…", "start": 0, "end": 0, "typed": "t",
      "expected": null }
  ]
}
```

- Two informational root keys precede `_meta`: `"version": 1` and `"spec"`
  (a one-line note). Loaders ignore them.
- `expected` for `enter` is the JSON literal `null` or
  `{ "location": int, "length": int, "replacement": string, "caret": int }`.
- `expected` for `capitalize` is the JSON literal `null` or a string.
- `null` is always the JSON literal (Swift `decodeIfPresent` / optional; Kotlin
  a null-value check; C# `JsonValueKind.Null`), **never** the string `"none"`
  (that convention belongs to `plot-vectors.json` and Rust's `Option`).
- `id` is unique across both arrays; `note` is free prose.
- `start`, `end`, `location`, `length`, `caret` are UTF-16 unit integers;
  `start > end` is allowed in a vector (reversed selection) and the port must
  normalise.
- `typed` is a JSON string containing exactly one scalar.

### 4.2 Encoding

The file is **ASCII-only**. Every unit outside U+0020–U+007E is written as a
`\uXXXX` escape — including `\r` (`\u000d`), `\t` (`\u0009`), U+00A0, U+200B,
U+2028, every Cyrillic and accented letter; a non-BMP scalar is written as its
surrogate-pair escape (`𐐨` for U+10428), also in `typed`. `\n` is
written as `\n`. The file MUST contain **no unpaired surrogate escape**
(`\ud800` alone): Swift's `JSONDecoder` rejects the whole document ("Missing
low code point in surrogate pair") and `System.Text.Json` rejects the string,
and a Swift `String` cannot hold a lone surrogate at all. `generate.ts`
refuses such a string. The lone-surrogate behaviour of §0.9 / §2.1a is
therefore a **native per-port test** on Kotlin and C# only (Swift has nothing
to test). Indentation 2 spaces, keys in the order shown, one vector per
object, so diffs stay readable and no editor can normalise an invisible unit.

### 4.3 Required coverage

`enter`: bullet / ordered / task continuation; `01.` → `2.`; `999999999.`
unchanged; `1) a` then Enter → `2) `; empty item terminate; `> - |` → `> `;
`> |` → ``; `> > |` → `> `; nested outdent to bullet, to ordered (`2. `), with
task box; nearest-ancestor choice (`- a / 4sp - b / 2sp - c / 6sp - d / 6sp -`
→ 2sp); tab-indented items (`\t- `); `>   - item`; `> > 1) item`; `-\tfoo`
(not a list → null); `- - -` (break → null); `- [x]\tdone` (box not
recognised → `\n- `); caret inside the prefix; caret mid-item (tail moves);
multi-line selection ending mid-item; selection covering the whole prefix;
table rows 1–5 of a five-row table (Enter on each); header-row Enter inserts
after the delimiter; mid-row caret; blank row ends the table; row after a
blank line after a table (null); single-column `| a |` / `---` table; escaped
pipe `a \\| b` cell counting; quoted table `> | a | b |`; table directly under
a prose line (continued — recorded divergence); `Title` / `---` (setext, null);
`foo | bar` / `---` (null); `-` / `foo | bar` (null); every front-matter guard
(`---` + blank line 1; `---` / `my title` / `---`; `+++` with `key = v`; valid
`---` / `title: x` / `---`); tentative front matter; fence opener line; fence
body; tab-led ```` ``` ```` (not a fence); 4-space ```` ``` ```` (not a fence);
```` ```js`x ```` (not a fence); closer with 6 leading SP; fence inside
`<!-- -->`; `> ```` (recorded divergence); comment block body; CRLF document;
lone-CR document; caret between CR and LF (null); empty document; caret at the
very end of a CRLF document; U+00A0 and U+200B blank lines; combining mark
after a marker.

`capitalize`: every row of §2.7; `Hello. w`; `„Hallo.“ e`; `«Привет.» о`;
`"Where?" s` → `S`; `"Go." s` → `S`; `Stop!!! n` → `N`; `… n` (U+2026) →
null; `E.G. n` → null; `Т.Е. n` → null; `ĞG. n` → `N`; `z. b` → null;
`p. e` → null; `см. рис. 3. д` → `Д`; `mail a@b.c. t` → `T`; `see [x](https://a.b). n`
→ `N`; `[x](https://a.|` → null; `2 < 3. n` → `N`; `<span class="a` → null;
`<!-- n` (comment line) → null; `\(a + t` → null; `\(x\). t` → `T`;
`$$` / `x` / `$$` / blank / `t` → `T`; unclosed `$$` in the paragraph → null;
`` `$` and so. n `` → `N` (code wins over math); `` ``a`` `` mid-span → null;
span opened on the previous line (per-line reset → capital); `- ́t` (mark
after marker; `t` typed after the mark → not at content start → null);
`Ⅻ. n` → `N` (Ⅻ is `Nl`, not a Letter); `ª` typed → null (`Lo`); `ⓐ` typed
→ null (`So`); ß, ŉ, ǰ typed → null; `ı` → `I`; `ǆ` → `Ǆ`; U+10428 → U+10400
(non-BMP typed and non-BMP `text`); `ა` (Georgian) → null; Arabic/Hebrew
letter → null; U+200B as the "previous
blank line" (irrelevant to A now, but the line containing only U+200B must be
*blank* for tableContext / P); table header being typed (`| n`) → null; body
row → null; `- 🔥 h` → `H`; `✅ d` → `D`; `— Куда? — с` → null; every
front-matter and fence case from `enter` with a typed letter; reversed
selection; selection replacing text mid-sentence; `first line` / `s` → `S`
(the bare-line-break decision, pinned).

Round-1 additions (both arrays): delimiter rows that are list items; the
narrowed tentative front matter (rule-then-list, closed-rejected block,
`key:` then list, CRLF); outdent to ordered/boxed/other-bullet ancestors,
the nearest ordered ancestor's number, TAB-vs-8-SP columns, a one-space
child, stoppers on a quote's inner text and on `> `; lists in quotes in
lists; tables nested under items and in double quotes, header column 0,
mid-delimiter caret, U+2028 cells; selections that reshape the caret's line
(box destroyed, fence closer deleted, marker created, spanning rows, inside
one row); CRLF / lone CR on every replace-head, fence, comment and table
path; only-marker documents; French spacing; `»`/`”` openers; the dash group
after a period; single capitals (`Plan B.`, initials chains, spaced
abbreviations); NFC/NFD initials; keycap and bullet leads; Greek
ypogegrammeni and MICRO SIGN; no HTML rule; `prefix2` on `before`; `P` cut at
block starts; Russian/Ukrainian dialogue and guillemet typography; other
scripts' terminators; sentence-final code spans, autolinks, footnote
references, Windows paths; nesting of fences, tables and quotes in items.

Round-2 additions (both arrays): greedy table consumption (a second pair
under a rule, the rule itself); column 0 of a body row (plain, quoted,
indented, CRLF), of the delimiter row and of a blank row; pasted bullets
and dialogue dashes as non-markers; bare markers at the line end (`-`,
`1.`, `2020.`, `- [ ]`, `---`); CRLF / lone CR on the mid-document table
row, selections across a terminator and inside a row, front matter,
comments and quote exits; quotes in items in quotes, spaced quote groups,
tables and fences in quotes in items with their blank-row exits; what
follows a table without a blank line (item, prose, comment, setext above);
escaped Windows-path pipes, TAB-indented delimiters, marks on the marker,
NBSP / U+3000 / BOM around markers, trailing inline comments; surrogate
pairs. `capitalize`: the Latin-1 fold; `?`/`!` runs over single letters and
abbreviations; emoji, keycap and hyphen groups between sentences; `’` as
an opener; footnote references and citations after the period; enumerator,
number and section leads with their negatives; WS19 after a marker;
display math and `](` inside quotes / code spans (§5); Russian guillemet
and dash typography; initials after markers; URLs, handles, paths and
hashtags at a sentence start; curly and single-guillemet openers, `¡`;
colon-then-quote; pipe-less tables; multi-line `\[ … \]`; discriminating
front-matter / comment pairs; CRLF selections between CR and LF, a
CRLF-only document.

Round-3 additions. `enter`: a blank row in the middle of a table (plain,
CRLF, quoted, column 0) and the exits that remain (nothing below, a blank
line, prose, another quote depth); a bare box `- [ ]` / `- [x]` / `1. [ ]` /
`> - [ ]` continuing the checklist and as an outdent ancestor; the front
matter field guard that mirrors the parser (`- key: v` lines, TOML `-` and
`#` lines, a TAB-indented YAML list, an indented closer); level-based
outdent (`- a` / ` - `, a 3-SP sibling, a 3-SP child); marker lines with a
pipe as delimiter rows (`- | -`, `- |`, `-- | --`, `- x`); header rows after
and inside items, at column 0 plain / quoted / nested, after the indent,
after the `>`; body rows at column 0 with rows below; selections that
reshape a header, a delimiter, a row's indent or quote prefix, that cross a
CRLF inside a table or from a parent into its empty child; escaped-pipe
cells, a heading with a pipe as a row, a BOM before a header, a comment with
a pipe inside a run, trailing spaces on header and delimiter rows, a
pipe-less quoted delimiter, CR-only and CRLF quoted rows; TAB-led fence
closers, fence openers at the caret inside items (2/3/4 SP); the ancestor
walk through a bare marker line, an emphasis line, two continuation lines,
a pipe continuation, and stopped by a quote line, a nested table, a
delimiter row; outdent inside a quote inside an item, with a quoted boxed
ordered ancestor, adjacent `>>` markers, a TAB child under a 2-SP parent,
mixed CR/CRLF; spaceless quotes at the prefix end and as siblings; `>\t`;
only-marker documents (`1) `, `\t- `, selected whole, CRLF after);
`999999999) `; NBSP content in a quoted item; U+000B inside content; `* * x`,
`- -`; Russian dialogue and guillemets inside items and quotes; `1) (see
a.)`, a quoted item ending in a URL; a hard break before a continuation
line; `1) [ ] a`; `- > q` and `- # h` on the marker line.
`capitalize`: enum leads after a footnote prefix, `[12]` citation leads and
their negatives; arrows as leads and not as a mid-line group; `--`, `---`,
`― `, `‒ ` leads and the mixed dash group; WS19 absorbed after every prefix
(NBSP after `>`, ZWSP, NBSP, U+3000 at a line start) and the BOM as content;
French spacing after guillemet openers at the line start, in an item, after
a sentence, with NBSP / U+202F, and the closer role of `»` / `«` after a
period; direct speech after a colon (`«`, `„`, `"`) and its negatives;
compact initials chains (`А.С.`, `И.И.`, `J.R.R.`, `U.S.A.`, `A.I.`) and
the non-chains (`n.b.`, `A.b.`, `J.R.R.?`); `я́` with a stress mark; nested
Russian quotes; `(Готово.) —` and `(Стой!) —`; `«тест». `; `…!`; `!!!` in
Cyrillic; a table row cutting the inline-scan paragraph; a sentence on the
line after a table; tables after two items, under a quoted item, indented
inside an item, inside a nested item after a blank line; `- | -` as a
delimiter row; a tilde fence inside an item not closed by backticks; the
fence closer line; `+++` alone; front matter with `- key: v` and TOML `- a`
lines; CRLF tentative front matter, code span, guillemets, quoted hard
break, blank-then-item, list-blank-table; a depth-2 quoted item; backslash
hard breaks; URLs with a trailing slash, a query string, an autolink before
the period, a link title, a bare domain; `[note].`, `` (`foo`). ``;
`*etc.*`, `**e.g.**`, `a.m` / `p.m`, `$HOME.`; a flag emoji, `◦`, `∙`; a
glued symbol; `1. [ ] `; `>\t- `; `- [` + `x` and the link-text contrast;
`ii.` / `iv.` / `i.`; a non-BMP capital; a closer after the space.

---

## 5. Recorded divergences from rendering

A divergence is allowed only if it is listed here with a vector id. A port that
finds another files it here first, JSON first.

| id | shape | what md renders | what the functions do | why |
| --- | --- | --- | --- | --- |
| `fence-in-quote` | `> ```` … | a code block inside the quote | not code: `enter` continues `> ` (wanted anyway), `capitalize` capitalizes | the state scan runs at quote depth 0 only (§0.10) |
| `table-after-paragraph` | prose line, then header + delimiter with no blank line | a paragraph of pipes (the paragraph loop does not break on tables) | a table: rows are continued, cells are not capitalized | classifying the block above the header would replicate the parser; the author's fix (a blank line) is the one the preview asks for |
| `table-mixed-quote` | `> a \| b` / `\|---\|---\|` | a 2-column table with a `> a` cell | not a table (quote depth differs) | the parser tests tables before quotes; the shape is a typo |
| `table-header-list-marker` | `- a \| b` / `\|---\|---\|` | a 2-column table with a `- a` cell (the parser tests tables before lists) | not a table: `enter` on the first line continues the list (`enter-table-header-with-marker-caret-header`), the delimiter line is prose (`enter-table-header-with-marker-caret-delimiter`), `capitalize` treats the first line as an item (`cap-table-header-with-marker-caret-header`) | §0.13 excludes a header with a `marker`; a list item that happens to contain a pipe must keep continuing as a list |
| `table-delimiter-list-marker` | `\| a \|` / `- ` | a 1-column table (a lone `-` is a valid delimiter cell) | not a table: the empty item terminates (`enter-table-delimiter-list-marker-dash-space`), a nested one outdents (`…-nested-outdent`), `- x` continues as a list (`…-text-not-table`); a bare `-` (`…-dash`, content per §0.11), `-\|`, `-:\|`, and every marker line whose content has a pipe (`- \|`, `- \| -`: `…-pipe-content`, `…-two-cells`) stay delimiter rows | §0.13 excludes a delimiter with a `marker` and no pipe: a writer who typed `- ` under a pipe line is starting a list, one who typed `- \| -` is building a table |
| `table-comment-row` | `\| a \|` / `---` / `<!-- a \| b -->` / `\| 2 \|` | a three-row table with the comment as a row (the row loop takes any line with a pipe) | the comment line is code and stops `top`: `\| 2 \|` is a pipe line, `enter` inserts a plain newline (`enter-table-comment-with-pipe-inside-run`) | one rule for comment lines; a comment with a pipe inside a table is a curiosity |
| `comment-in-quote` | `> <!--` … | a comment block inside the quote (the quote's inner text is parsed recursively) | not code: `enter` continues `> - ` (`enter-comment-in-quote-not-comment`), `capitalize` capitalizes (`cap-comment-in-quote-divergence`) | the state scan runs at quote depth 0 only (§0.10) |
| `comment-closed-same-line-prose` | `<!-- x --> yy. n` | the parser drops the text after `-->` on a comment line | not capitalized (the line is code) — the same answer (`cap-comment-closed-same-line-prose`) | one rule for all comment lines |
| `list-continuation-line` | `- item` / `text` (no blank line) | one item, "item text" joined by a space | `text` is a line start → `Text` | §2.4: every line start is a paragraph start; in md's own flow the second line is almost always a new paragraph the author forgot to blank-separate |
| `footnote-continuation-line` | `[^1]: note` / `more` | one footnote, joined | `More` | same as above |
| `code-span-per-line` | a code span opened on one line, closed on the next | one span (HTML renderer) | the span resets at the line start | typing-time approximation (§2.1) |
| `code-span-run-length` | ``` ``a`b`` ``` | the HTML renderer's single-backtick regex mangles it; the native view shows one span | one span, CommonMark run-length | follows the native view and CommonMark |
| `comment-note-prose` | `<!-- note: my note. it says -->` | shown in the notes panel | not capitalized (comment = code) | one rule for all comments; notes are private |
| `math-in-inline-code-priority` | `$…` then backticks inside | code protected first | while `inline` is open, backticks are literal | pinned by vector, not worth a third state |
| `display-math-in-quote` | `> $$` / `> x` / `> $$ y.` | the quote's inner text is parsed recursively, so `$$` spans quote lines | `P` is cut at every quote line: `> $$` / `> ` capitalizes (`cap-quote-display-math-open`), the closer line reads as an opener (`cap-quote-display-math-closed-then-sentence`) | a per-depth `P` would replicate the recursive quote parse; fences and comments in quotes are already recorded the same way |
| `link-destination-raw-search` | `` `](`. n `` | the `](` is literal code | precondition §2.1e is a raw search over `before`: `null` (`cap-link-paren-in-code-span`) | one raw search instead of a per-unit "normal" mask; the shape is a tech-note curiosity |
| `bare-marker-line` | `-`, `1.`, `2020.` alone on a line | an empty list item numbered as written | content: `enter` inserts a plain newline, `capitalize` treats the unit as a token; under a pipe line a bare `-` is a delimiter row (`enter-table-delimiter-list-marker-dash`) | Enter must not delete a `2020.` line; every editor compared requires the SP (A.1) |

---

## 6. Differences from CommonMark, by design

Inherited from `MarkdownParser`, restated so a porter does not "fix" one side
toward a platform Markdown library:

- A TAB after a list marker is not a list (`-\tfoo` is a paragraph); only SP.
  A bare marker at the line end (`-`, `2020.`) is an empty item to the parser
  but content to both functions (§0.11, §5 `bare-marker-line`). A bare box
  (`- [ ]`) is an empty task to the parser; it is content for the caret but
  `enter` continues it as a task (§0.11).
- List nesting is by level `indent / 2`, TAB counting to the next 4-column
  stop: ` - b` under `- a` and `   - c` under `  - b` are siblings, and
  §1.3a outdents by level.
- No block nesting inside a list item on the marker line: `- # h` and `- > q`
  are items whose text is `# h` / `> q` (`ListItem.text` is inline-only), so
  the typed letter after `- # ` is not the item's first content unit and stays
  lowercase.
- Any indentation (not ≤ 3) before `#`, `>`, list markers and rules is fine;
  the fence opener alone is limited to ≤ 3 SP.
- No lazy continuation for quotes: a line without `>` ends the quote.
- No loose lists: a blank line always ends a list (hence every upward walk
  stops at a blank line).
- No indented code blocks: `    text` is a paragraph, `    - c` a nested item.
  `\t# h` is a paragraph (`parseHeading` drops SP only), `\t- c` a list item.
- A fence opener needs ≤ 3 leading SP even inside a list item: `- a` / `    ```` `
  (4 SP) is item text, not a fence, and is capitalized
  (`cap-nest-fence-4sp-in-item-not-fence`); with 2–3 SP it is a fence that ends
  the item.
- No HTML blocks, no link-reference definitions (`[foo]: url` is prose).
- The paragraph loop does not break on a table; a table needs a blank line or a
  block boundary before it (§5 `table-after-paragraph`).
- Every newline inside a paragraph renders as `<br>`; two-trailing-space and
  backslash hard breaks are irrelevant, a lone `\` at a line end is literal.
  This is why §2.4 capitalizes every line start.
- Ordered lists render the literal ordinal; nothing renumbers (§1.4).
- Setext underlines apply only to a one-line paragraph; `---` after two or more
  lines is a thematic break, `===` is text. (No rule here depends on it any
  more; noted for the outline and for §5.)
- `$$…$$` and `\[…\]` are inline spans inside a paragraph, not blocks; a lone
  `$$` line is literal text once a blank line ends the paragraph.
- Front matter needs all four guards (§0.10); a document opening with a rule,
  prose and a rule is prose, and a rule followed by a list or a heading is a
  rule and that block (the tentative rule needs a `key:` line first).

---

## Appendix A — Decisions

### A.1 Contested points, decided

| point | decision | reason |
| --- | --- | --- |
| Line start = paragraph start (A.b/A.c boundary list vs "always") | **always** (§2.4) | md renders `<br>` per newline; all four keyboards capitalized after Return before; removes the whole boundary enumeration (setext, quote depth, `\newpage`, comment end) — fewer places for three ports to disagree. Critic 1's quote objection (`> The quick` / `> brown`) is a hard-wrap habit md displays as two lines anyway. |
| `$$` block state | deleted; paragraph-scoped inline scan (§2.1) | no such block exists in the parser; the toggle poisoned the rest of the document |
| Table membership | header + delimiter + contiguous pipe lines (§0.13) | the parser's rule; adjacency to the delimiter died on row 3 |
| Table membership in quotes | applied on quote-stripped inner text, same depth | cheap; `> \| a \|` tables are real |
| Header row Enter | insert after the delimiter row | inserting between header and delimiter destroys the table |
| Mid-row Enter | insert a new row after the row (empty selection only) | Typora / Notes / GitHub behaviour; `null` would silently split a row |
| New-row style | always `\|  \|  \|` | one form; renders identically; a pipe-less one-column row would be an empty line |
| `N` | header cell count | the renderer pads every row to the header |
| Ordered "keep 1" | dropped: `next = n + 1` | every renderer prints the literal ordinal |
| 9-digit overflow | keep the current number | the parser rejects a 10-digit marker |
| Outdent task box | kept iff the ancestor has one | checklists stay checklists |
| Empty nested quote | drop one level (`> > ` → `> `) | symmetric with list outdent |
| Multi-paragraph quotes (`>` kept on an empty quote line) | rejected; empty quote line exits | majority behaviour (Typora, Obsidian, iA, Notes); a `>` typed by hand is cheap |
| Renumbering below an inserted item | not in v1 | the edit contract has one range; the preview is unaffected |
| Front matter | the parser's four guards + tentative rule | `---⏎t` must not become `Title:`; a rule-prose-rule document must stay prose |
| Indented code precondition | deleted | md has no indented code |
| Fences in quotes | not detected, recorded | avoids per-depth fence state in the one pass |
| Comment blocks | code state for both functions | one rule; `<!-- note:` prose is private |
| Inline math | renderer's currency guard folded into the scan | `$5. the` must capitalize; unmatched `$x` still opens |
| Code spans | run-length, per line | native view + CommonMark; double backticks are common in tech notes |
| HTML `<` | only `<` + Letter / `/` / `!` / `?` | `a < b` is prose |
| Reference definition `]:` precondition | deleted; footnote prefix added | md has no reference definitions; footnotes are real |
| B.4(iv) URL token before the terminator | deleted | a URL never contains `. `; the current-token rule covers the inside |
| Ellipsis | terminator run with ≥ 2 `.` | consistent with the run mechanism; `?..`/`!..` included |
| Single-letter tokens | a lowercase single letter is an abbreviation or numbered unit; an uppercase one is an initial only after another single-letter token; U+044F `я` alone is exempt (§2.5 ii) | `т. е.`, `z. B.`, `с. 15`, `J. R.` covered by one rule; `Plan B. Then` and `Я. О` capitalize |
| Abbreviation fold | ASCII + Cyrillic (+ Є І Ї Ґ) only, ordinal compare | locale/full lowercasing differs per runtime |
| Uppercasing | simple mapping via the one-scalar guard; no titlecase | .NET has no per-rune titlecase; `ǆ` is academic |
| Georgian | null | no sentence capitalization in the script; Mtavruli mapping varies by ICU version |
| Dialogue dash / emoji leads | added to `prefix2` | Russian prose and status notes; cheap |
| Table cells | neither A nor B; pipe lines too | cells hold identifiers; consistent before and after the delimiter row exists |
| Trigger | one scalar in the function; word-insertion reduction in the adapter | QuickPath / predictive / Gboard composition all arrive as words |
| iOS `shouldChangeTextIn` | replaced by `insertText` override | fires for autocorrect and programmatic paths too |
| macOS Shift-Return | `insertNewline:` with a modifier check; `insertLineBreak:` is Ctrl-Return | verified in `StandardKeyBinding.dict` |
| Undo | separate undo step (Apple, Windows); atomic on Android, recorded | word-processor reflex; Android's `InputTransformation` cannot split it |
| Override | armed until an insertion elsewhere | the draft's one-backspace gesture was undiscoverable and looped |
| Vector `null` | JSON literal | Swift/Kotlin/C# loaders all handle it; `"none"` was a Rust artefact |
| Delimiter row with a list marker (`\| a \|` / `- `) | not a table when the marker's content has no pipe; `- \| -` and `- \|` are delimiter rows (§0.13, round 3) | the writer typed a list item; one who typed a pipe under a pipe line is building a table, and md renders it; §5 `table-delimiter-list-marker` |
| Tentative front matter under a bare `---` | narrowed (§0.10): a closed rejected block is prose; `#`/`-` lines are field-like only under a field; `enter` tests the caret's line; round 3: a field never starts with `#` or `-` (the parser's guard), and the `-` clause is YAML-only | `---⏎- a⏎- b` was a dead list for both functions; `---⏎tags:⏎- a` stays YAML; `---⏎- key: v⏎---` can never become front matter, so its lines are Markdown; TOML has no `-` lists |
| Enter at column 0 of a table header | `null` | the only way to insert a line above a table; a body row at column 0 still inserts a row (a plain newline there would end the table) |
| Blank row inside a quoted table | ends to `indent0 + quotes` | symmetric with §1.3b; a blank line would also end the quote |
| Enter with a selection inside one table row | delete the selection and add a row, one range (§1.1) | `null` tore the row in two; the blank-row exit needs an empty selection |
| Caret inside the SP run an empty marker absorbs (`- \|  `) | `null` (§1.2 first) | invisible trailing spaces; a plain newline is harmless and the rule order stays one-directional |
| Ancestor walk over `> ` (blank inner text) | stops | the renderer ends the list at the blank inner line |
| HTML-tag precondition | deleted | md renders no inline HTML; `if a<bb. Then` is common technical prose |
| `»` and `”` as openers | added | German/Danish `»…«` and Swedish/Finnish `”…”` (`’` joined them in round 2, see below) |
| Dash group after a period (`он. — Пойду`) | capitalizes; after `?`/`!` stays a tag | standard Russian dialogue punctuation; `Done. — Yes` in English |
| Single uppercase letter before the terminator | an initial only after another single-letter token | `Plan B. Then`, `vitamin C. It` are far more common than a lowercase word after a lone initial |
| Letter + Marks token | one letter | NFC and NFD `é.` must agree; Swift compares canonically |
| French spacing before `!`/`?` | look back over exactly one WS19 unit | every French question ended with an empty token |
| Keycap emoji (`1️⃣`) | a symbol lead | numbered bullets by keycap are the case leads were added for |
| Pasted bullets `•` `‣` `·` `◦` | in the dash group | Notes/Pages/Word paste them; arrows joined in round 3 (below) |
| `prefix2` measured on `before` | yes (§2.3) | `- \| item` inserts the first content unit |
| Inline-scan paragraph `P` | cut at quote/marker lines; breaks and page breaks end it | the parser's paragraph loop breaks there; the fix reuses §1.3a's line tests |
| Terminator inside link text (`[Done.](x) then`) | not a sentence end | a link is one token; `(x)` is a destination |
| List-ending and unit-style abbreviations (`etc`, `usw`, `т.д`, `г`, `руб`) | stay in the list | a missing capital costs one Shift; decided once so four ports agree |
| NBSP after `>` (`>\u00A0t`) | `quotes` = `>` (§0.11 takes one SP), and `prefix2` absorbs the NBSP: `T` (round 3 reversed the round-1 `null`) | the NBSP is invisible in the rendering, exactly like `- \u00A0t`, which already capitalized; one rule for every prefix (§2.3) |
| Indented list continuation (`- a` / `  t`) | capitalized (rule A, "always") | one rule; a wrapped item is a hard-wrap habit md displays as two lines anyway |
| Greek ypogegrammeni, MICRO SIGN | `upper` undefined | Swift/JS full mapping vs Java/.NET simple mapping differ; `µs` must stay |
| Table membership when a pipe-less line follows a pair (`\| a \|` / `---` / `\| 1 \|` / `---` / `\| x \|`) | greedy consumption (§0.13): the first pair owns every pipe line under it; the search resumes below the line that ended it | matches the parser's row loop; a second pair inside the first table's rows made `enter` insert a row under what the preview shows as a rule |
| Enter at column 0 of a body row | insert the blank row **above**, caret in it (§1.1) | the caret's row moves down as a plain newline would push it, and "insert a row above" has no other gesture; the delimiter row still inserts below; a blank row exits whatever the column |
| Pasted bullets `•` `‣` `·` and dialogue dashes for `enter` | not markers: plain newline | md renders `• item` as a paragraph; continuing it would manufacture more non-Markdown; they stay `capitalize` leads |
| Marker or box at the line end (`-`, `1.`, `2020.`, `- [ ]`) | content for both functions (§0.11); a bare box also counts as a box for §1.3a / §1.4 (round 3) | `2020.` + Enter deleted the year; every editor compared (Typora, iA, Obsidian, VS Code) requires the SP; `- [ ]` + Enter continues with `- [ ] ` because the parser renders an empty task and nothing is deleted |
| Caret between the halves of a surrogate pair | `null`, both functions (§0.9) | symmetric with the CR-LF rule; splitting a pair corrupts the text and the three bridges repair it differently |
| Latin-1 capitals in the fold | folded, U+00C0–U+00DE except `×` (§2.6) | `o.Ä.` is the Duden spelling; one-to-one and identical on every runtime, like the Cyrillic range |
| `?` / `!` runs over a single letter or an abbreviation | capitalize: (ii) and (iii) apply to the single `.` only (§2.5) | `what is x? I`, `etc.? Next`: abbreviations and initials never end in `?` or `!` |
| Emoji / keycap between two sentences (`Done. 🎉 next`) | a symbols group in step 1b, no `.` restriction (§2.5) | the status-note leads, mid-line; `Done 🎉 next` still needs a terminator |
| Hyphen-minus as a mid-line dialogue dash (`он. - Пойду`, `Done. -- yes`) | in the step-1b dash group; a run of ≥ 2 hyphens is also a `lead` since round 3 (`-- Привет`) | Android/Windows keyboards have no em dash; at a line start a single `- ` stays a list marker, `--` is neither a marker nor a break |
| `’` (U+2019) | an opener as well as a closer (§2.2) | smart punctuation and Swedish/Finnish typography produce it as an opening mark; after WS19 it is never a contraction |
| Footnote reference / citation after the period (`text.[^1] Next`, `text. [1] Next`) | skipped like a closer (§2.5 step 2) | Chicago style; `[x]` stays a plain closer |
| Enumerators `(1)`, `(a)`, `(iv)`, `a)`, `б)` at a line start | an `enum` lead, first lead only (§2.3) | legal / academic drafting; mid-line `see (a)` unchanged |
| Numbered headings `## 1.1 x`, `## 1) x`, `## 2 x` | a `number` lead after `headingPrefix` only (§2.3) | `## 1. x` already capitalized by rule B; the same lead at paragraph level would capitalize `2.5 cups` (A.2) |
| `§ 3 x`, `§3 x`, `§ x`, `¶ x` | `section` lead / `§` `¶` in the dash group (§2.3) | German, Austrian and Russian legal notes; U+00A7 is `Po` since Unicode 6.1 so the So rule missed it |
| WS19 after a marker's SP (`- \tt`, `- \u00A0t`) | absorbed into `prefix2` (§2.3); since round 3 after **every** prefix, including none (`\u200Bt`, `>\u00A0t`) | the parser drops SP only, the rest is invisible in the rendering; symmetric with `headingPrefix`; the grammar and the prose had disagreed |
| Display math across quote lines | recorded divergence, `P` stays per quote line (§5 `display-math-in-quote`) | a per-depth paragraph would replicate the recursive quote parse; fences and comments in quotes are recorded the same way |
| Pasted / predicted URL, path or handle as a word insertion | the adapter applies §2.1f to the whole insertion and skips (§3.3) | the pure function sees one scalar; `Https://a.b` must never be inserted |
| Blank row in the middle of a table (round 3) | an ordinary row: the exit fires only when the table does not continue below (§1.1) | ending the table mid-way orphaned every row below it as a paragraph of pipes; a list has no such cost because two lists render fine |
| Outdent ancestor by level, not by column (round 3) | `floor(columns / 2)` strictly smaller (§1.3a) | the parser nests by `indent / 2`, so ` - b` and `   - c` render as siblings; Enter on an empty sibling must exit, not manufacture another |
| Below-caret reads (round 3) | exactly three, named in §0.14: the front-matter closer, the delimiter row under a header, the "table continues below" line | a port that implemented "never look below the caret" literally failed nine header-row vectors |
| `enum` / `citation` after a footnote prefix (round 3) | first leads after `prefix`, `headingPrefix` or `footnotePrefix` (§2.3) | `[^3]: (a) The parties` in legal drafting; the reference already did it and the text did not say so |
| `[12]` at a content start (round 3) | a `citation` lead (§2.3) | reference lists in academic notes are typed this way; mid-line `see [12] then` is unchanged |
| French spacing after an opening guillemet (round 3) | the WS19 run after `«` `‹` `»` `›` belongs to the opener run when the guillemet is preceded by WS19, an opener or only `prefix2` (§2.2) | `« Bonjour »` got a capital after a period but not at a line start — the opposite of every keyboard; the precondition keeps `Done.» then` and `»Hallo.« dann` on the closer path |
| `--` / `---` as a dialogue-dash lead (round 3) | a run of ≥ 2 U+002D is a `dash` lead; a single hyphen never is (§2.3) | Russian dialogue on Android/Windows keyboards; `--` is neither a marker nor a break; `--- t` is not a break once it has content, so the capital there is harmless |
| U+2015 HORIZONTAL BAR, U+2012 FIGURE DASH (round 3) | in `dashUnit`, so leads and the mid-line group (§2.3, §2.5) | U+2015 is Unicode's "quotation dash": OCR and e-book paste produce it on every dialogue line |
| Arrows `→` `⇒` `←` (U+2190–U+21FF) as leads (round 3; reverses the round-1 A.2 rejection) | in `dash` for the content start only, not in the mid-line group (§2.3) | at a content start an arrow cannot continue an expression; arrow bullets are a common note style; `➡️` (So) already led while `→` (Sm) did not, and a writer cannot tell which their keyboard emitted; `a → b. c` is untouched |
| Direct speech after a colon (round 3; reverses the round-2 pin, now `cap-colon-then-quote-direct-speech`) | `:` + WS19 + a quotation opener capitalizes, no token tests (§2.5 1a) | Розенталь §49 and every English style guide; `note: the` and `He said: (` are untouched; `the word he used: "foo"` is the accepted cost |
| Mixed dash run `—-` (round 3) | one run of units each of which is a dash unit or U+002D (§2.5 1b) | the text's "a run of dash units or of U+002D" had two readings; the reference's is pinned |
| Table rows cut the inline-scan paragraph (round 3) | a table row is a paragraph end like a special line (§2.1) | the parser's row loop consumed the rows; an unclosed `$$` in a cell must not silence the prose below; the run is already bounded |
| Compact initials `И.И.`, `J.R.R.`, `U.S.A.` (round 3) | an initials chain: ≥ 2 groups of one Uppercase letter (+ Marks) joined by `.` → `null` (§2.5 ii′) | the GOST spelling of every Russian bibliography; `U.S.A. Then` flips to a miss, consistent with `U.S.` in the list; `A.I. Then` is an accepted miss; lowercase groups stay ordinary (`n.b.`) |
| Lone surrogates in the JSON (round 3) | removed; `generate.ts` refuses them; the behaviour is a native Kotlin/C# test (§4.2) | Swift's `JSONDecoder` rejected the whole 1146-vector document; `System.Text.Json` rejected the five strings; a Swift `String` cannot hold one |
| `- [` + `x` (round 3) | `null` when `[` is the first content unit after a marker and the typed letter is `x` (§2.4) | typing a checklist by hand turned `[x]` into `[X]` on every checked item; any other letter after `- [` is link text and is capitalized |
| Roman / lettered enumerators with a period (`i.` vs `ii.`) (round 3) | pinned as-is: `i. `, `a. ` are single letters (`null`), `ii. `, `iv. `, `A. ` capitalize (§2.6) | a `Letter "."` lead would capitalize `т. е.` and `z. B.` at a line start; the inconsistency is rare and visible |
| BOM invariant (round 3) | stated in §3.3: the editor text never begins with U+FEFF, the functions treat it as content | the loaders already strip it; a future loader change would silently kill both features on line 0 |
| First hand-typed body row of a pipe-less table (`a \| b` / `--\|--` / `x`) (round 3) | accepted cost: the first cell is capitalized until its `\|` is typed (`cap-table-pipeless-body-start`); Enter avoids it | a "row in progress" rule for pipe-less tables would also uncapitalize the prose line under every table, which is far more common |
| `a.m` / `p.m` (round 3) | stay in §2.6 | `from 5 p.m. to 6 p.m.` is as common as `at 5 p.m. Then`; the list-ending rule applies |
| Comment line with a pipe inside a table run (round 3) | recorded divergence (§5 `table-comment-row`): the comment stops `top`, the row below is a pipe line | letting `top` walk through code lines for one curiosity is not worth a rule |
| Override (v1.1; supersedes "armed until an insertion elsewhere" above) | tracks md's capital through later edits; deleting it — however — arms the retype (§3.4) | the old "armed until an insertion elsewhere" rule contradicted its own `Md is` example (the `d` typed after the capital had already disarmed it) and was refuted by reviewers on iOS, macOS and Windows |

### A.2 Minor suggestions rejected

| suggestion | why not |
| --- | --- |
| Lettered-enumerator auto-revert (`a)` → lowercase again) | a retroactive edit outside the pure-function contract; the override covers it; revisit with a vector if it bites |
| Brand-name retro-fix list (`Md` → `md`, `Ios` → `iOS`) | a curated list to maintain in four ports for a case iOS autocorrect already fixes; override elsewhere |
| Tab in a table row jumps to the next cell | a separate platform gesture, not part of `enter`; worth its own issue |
| Optional extra edits in `EnterEdit` for renumbering | v1 keeps one range; a later spec version may add it JSON-first |
| Multi-paragraph quote refinement | see A.1 |
| Keep `KeyboardCapitalization.Sentences` permanently on Android | two rules would fight in fences; it stays only until the transformation is verified (§3.2) |
| Drive iOS `autocapitalizationType` dynamically per context | keeps native visuals but lets iOS's own quirks (capital after `...`, no Russian abbreviations) back in on one platform; md owns the rule (§3.2) |
| "First non-space after an unescaped `\|` is a paragraph start" | cells hold identifiers; decided the other way (A.1) |
| Compute fence state on quote-stripped inner runs | rejected for the one-pass budget; recorded divergence instead |
| Titlecase mapping where it differs from uppercase | no per-rune API in .NET; academic |
| Treat a lone `$` before a space as opening math "in progress" | `$ ` never opens: the renderer's guard says so and prices are common |
| A closer right after an abbreviation ends the sentence (`(etc.) then` → `Then`) | `the word "etc." is Latin`, `called "Mr." by` would capitalize; the closer alone is not evidence; pinned `null` |
| Extend `enum` to `[ivxIVX]{1,4} "."` and `Letter "."` at a line start (so `i.` and `ii.` agree) | `т. е.`, `z. B.` and `с. 15` at a line start would gain a capital on the wrong unit; pinned as an accepted inconsistency instead (A.1, round 3) |
| "Row in progress" for a pipe-less line directly under a table (uncapitalize `a \| b` / `--\|--` / `x`) | the same rule would uncapitalize the prose line after every table (`\| 1 \|` / `After.`), which is far more common; accepted cost recorded in A.1 (round 3) |
| Let `top` walk through a comment line that contains a pipe (follow the parser's row loop) | a code line inside a table run is a curiosity; recorded in §5 `table-comment-row` (round 3) |
| Treat `[` as never an opener when it is the first content unit after a marker (general form of the `- [` + `x` rule) | `- [Home](…)` link lists are common and want the capital; only the typed `x` is the box (round 3) |
| Strip the WS19 run after every opener (`( t`) | only guillemets carry French spacing; `( ` is a typo (round 3) |
| Add arrows to the mid-line dash group (`a. → b`) | `a → b. c` is an expression; leads only (round 3) |
| Empty-item test before §1.2 when the caret sits in the absorbed SP run | needs a per-shape "marker units end" offset for one invisible case; `null` is harmless |
| U+3002 / U+FF01 (CJK, fullwidth) and U+0589 (Armenian) as terminators | not in v1; scripts without sentence casing; JSON-first |
| `©`, `°` leads (`© nettrash` → `© Nettrash`) | accepted: So leads are one rule; the override covers it |
| Skip §1.3a's code stop for a fence indented inside the item | the parser ends the item at the fence too (`FenceMarker` accepts ≤ 3 SP); the terminate is consistent with the preview |
| Enter at the end of a lone header row completes the table (`\| a \| b \|` → delimiter + empty row) | a feature, not a bug: Obsidian, iA, Bear and VS Code do not; Typora does it through its table UI. `null` stays (`enter-table-pipe-line-alone`); listed for nettrash as a candidate for a later version, JSON-first |
| Enter at the end of a fence opener auto-closes the fence | same: no Markdown editor compared does it, and it needs a forward scan below the caret that §0.14 does not budget for. `null` stays (`enter-fence-opener-line`); a candidate for a later version |
| `number` lead at paragraph / item level (`1.1 Scope`, `- 3.5 kg`) | in md's own documents a leading number is a quantity far more often than a section number (`2.5 cups`); headings only (`cap-para-number-no-dot`, `cap-item-decimal-not-lead`) |
| Per-depth paragraph `P` for display math inside quotes | replicates the recursive quote parse for one rare shape; recorded in §5 instead |

### A.3 Provenance

Rules were checked against `md/md/MarkdownParser.swift` (identical to
`md.macOS`), `md.Android/…/markdown/MarkdownParser.kt`,
`md.win/src/Md.Core/Markdown/MarkdownParser.cs`, `md/md/MarkdownHTML.swift`
(inline protect order and the currency guard), `md/md/ScalarText.swift` and
the four editors (`md/md/MarkdownEditor.swift`,
`md.macOS/md/MarkdownEditor.swift`, `md.Android/…/ui/EditorScreen.kt`,
`md.win/src/Md.App/Controls/EditorPane.cs` + `Md.App.Logic/Documents/EditorText.cs`,
`Md.App.Logic/Settings/SettingsKeys.cs`), and `plot-vectors.json` for the
vector-file conventions.
