# md for Linux — the port plan

md for Linux is md 1.6 for Ubuntu 26.04: a Rust + GTK 4 / libadwaita / WebKitGTK 6.0 app with
**the functionality of md for macOS**, the family's source of truth, installed from a `.deb`.

## The reference: md.win, port of md.macOS

md.win (`../md.win`, C#, .NET 10) is a faithful, fully tested port of md.macOS, and is the
reference this port is written against:

- `md.win/src/Md.Core` (≈ 12 000 lines) — parser, HTML writer, stylesheet, plots, exports, books,
  codec, view-mode memory, editing rules. Pinned byte for byte to md.macOS by the fixtures every
  md port shares, and by ≈ 2 800 tests in `md.win/tests/Md.Core.Tests`.
- `md.win/src/Md.App.Logic` (≈ 7 000 lines) — everything the shell does that is testable without
  a display: documents and autosave, commands and menus, view modes, Zen, the preview
  coordinator, the export pipeline, typing hooks, settings, session. ≈ 1 600 tests.
- `md.win/docs/` — `core-api.md` (the Core surface), `shell-design.md` (the Mac shell mapped onto a
  desktop toolkit, with the Mac's behaviour spelled out section by section and §14 listing every
  deliberate deviation), `smart-typing.md`, `formatting.md`.
- `md.macOS` (`../md.macOS`, Swift/AppKit) — consult it when md.win's behaviour or comment is
  unclear; the Mac wins.

Read md.win and md.macOS; **never modify either repo**.

## Layout

| Path | What | Port of |
| --- | --- | --- |
| `crates/md-core` | the parity core, std + `unicode-segmentation` only | `Md.Core` |
| `crates/md-logic` | the shell's logic, no GTK | `Md.App.Logic` |
| `app` (binary `md`) | GTK 4 / libadwaita / WebKitGTK shell | `Md.App` (WinUI) / the Mac app |
| `rich/` | KaTeX, mhchem, Mermaid, Viz.js, PlantUML, highlight.js, `md-init.js` — byte-identical to md.win's `src/Md.App/rich` | |
| `examples/` | the bundled examples and Example Book — identical to md.macOS's | |
| `crates/md-core/tests/fixtures` | the shared fixtures, byte-identical to md.win's | |
| `tools/unicode-tables` | generator of `text/unicode_tables.rs` from .NET | |

md-core modules and their md.win sources:

| Module | md.win source |
| --- | --- |
| `text` (done) | `Text/Whitespace.cs`, `Text/ScalarText.cs`, + .NET's Unicode answers, UTF-16 offsets, graphemes |
| `document` | `Document/*` (`plain_text_codec` done) |
| `markdown` | `Markdown/*` except `Plot*` |
| `plot` | `Markdown/Plot.cs`, `PlotMemo.cs`, `PlotInternals.cs` |
| `editing` | `Text/SmartTyping.cs`, `Text/MarkdownFormatting.cs` |
| `book` | `Book/*` |
| `export` | `Export/*` |

## Rules for every port

**Names.** md.win's names in Rust spelling: `MarkdownParser.Parse` → `markdown::parser::parse`,
`BlockKind.Heading` → `BlockKind::Heading`, a C# record → a struct or enum with the same fields.
One Rust file per C# file where that reads naturally. Keep md.win's comments that explain *why*
(they are the family's memory of each divergence), adapted to Rust.

**Strings.** C# strings are UTF-16 code units; Rust's are UTF-8.

- md-core's `&str` APIs take and return **byte offsets on char boundaries** unless the name says
  `utf16`. Search with ASCII delimiters and slice at the offsets found; never `s[i..i + 1]` on a
  position that was not produced by a char walk or an ASCII match — slicing off a boundary
  *panics*, and "Привет" in a document is enough to find it (it crashed family.connect's server).
- Where md.win's behaviour depends on UTF-16 counts (an offset in the API, a length compared to a
  limit, `Substring(0, n)` with `n` in units), port it with `text::utf16`.
- The editing rules (`editing`) take and return UTF-16 offsets exactly as the vectors spell them;
  port them on `text::utf16::Utf16Text` (a `Vec<u16>`) so their index arithmetic reads line for
  line like the C#.
- Valid Rust strings hold no lone surrogates. Where md.win carefully handles one, keep the
  behaviour for valid input and say in a comment that the case cannot arise.

**Unicode.** Never `char::is_whitespace`, `char::is_alphabetic`, `char::to_uppercase`,
`str::trim` or `str::to_lowercase` in ported code. Use `text::whitespace` (the five sets),
`text::unicode` (.NET's general categories and simple case mappings: `Rune.IsLetter` →
`unicode::is_letter`, `ToUpperInvariant` → `unicode::to_upper_invariant`, `char.IsWhiteSpace` →
`unicode::is_dotnet_white_space`), `text::scalar_text::full_lowercase` and `text::graphemes`
(`StringInfo`). `string.Trim()` in md.win is almost always `Whitespace.TrimWS`; check which.

**Numbers.** .NET formats a `double` with `ToString(CultureInfo.InvariantCulture)` as the shortest
round-trip string, switching to `E` notation (`1E+21`, `1E-07`); Rust's `{}` never uses an
exponent. `Math.Round` is banker's rounding unless `MidpointRounding.AwayFromZero` is passed;
Rust's `round` is away from zero. `pow(10, n)` is not portable across libms — parse the decimal
literal instead (md.vscode's Node 20 CI found 68 of 632 exponents wrong). Write a helper with
tests wherever the C# formats or rounds; never assume `{}` matches.

**Regex.** md.win uses `System.Text.RegularExpressions` in a few places; md-core has no regex
crate. Hand-write the matcher and port the tests that pin it.

**Dependencies.** md-core: std and `unicode-segmentation` only. md-logic: md-core and std. A
hand-written SHA-1/SHA-256/CRC-32/inflate is expected, each tested against published vectors.
Dev-dependencies: `serde_json` for reading the vector files. Anything else: ask first.

**Toolchain.** Must build with Ubuntu 26.04's rustc 1.93 (edition 2024): use no std API
stabilised after 1.93. `cargo fmt --all`, and `cargo clippy --all-targets -- -D warnings` must be
clean: **zero warnings** in any build, tests included. No `unsafe` (`forbid`).

**Tests.** Port **every** md.win test of the module, same name in snake_case, same inputs, same
assertions, as integration tests in `crates/md-core/tests/<module>_*.rs` (helpers in
`tests/common`). Golden files and vectors are compared byte for byte; **never edit a fixture**. A
test that cannot pass on Linux by nature is ported with `#[ignore = "<reason>"]` and listed in the
report. A test that fails because the port is wrong is a bug: fix the port.

**Git.** Commit on your own branch only, author `nettrash <nettrash@nettrash.me>` (the repo's
config), message in the repo's style: a short summary line, then a paragraph. **No**
`Co-Authored-By`, no "Generated with", no mention of any AI or tool. Never push, never merge,
never tag.
