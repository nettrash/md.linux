//! Port of md.win `tests/Md.Core.Tests/DelimitedTableTests.cs`: the CSV / TSV table builder
//! shared by the HTML and LaTeX writers. What counts as a number decides column alignment, and it
//! must mean the same thing on every platform — which is why nothing here goes through a
//! standard-library parser.

use md_core::markdown::{ColumnAlignment, DelimitedTable};

use ColumnAlignment::{Leading, Trailing};

fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter().map(|row| row.iter().map(|cell| cell.to_string()).collect()).collect()
}

fn alignments(code: &str, separator: char) -> Vec<ColumnAlignment> {
    DelimitedTable::from(code, separator).expect("a table").alignments
}

#[test]
fn decimal_number_grammar_is_explicit_not_double_init() {
    // Swift's Double takes hex and any casing of inf/nan, Java's parseDouble takes neither, .NET's
    // double.TryParse takes thousands separators and culture decimals, Rust's f64 parse takes inf.
    for number in ["0", "-1", "+2", "3.5", ".5", "5.", "1e9", "-2.5E-3", "007"] {
        assert!(DelimitedTable::is_decimal_number(number), "{number:?}");
    }
    for other in [
        "0x10", "inf", "Inf", "INF", "nan", "NaN", "1,000", "1 000", "", "-", ".", "e5", "1e", "1e+", "12abc", "1.2.3",
        "٣", "½", " 1", "1 ", "+", "1e5.0", "1E-", "Infinity", "1_000",
    ] {
        assert!(!DelimitedTable::is_decimal_number(other), "{other:?}");
    }
}

#[test]
fn delimited_parsing_follows_rfc4180() {
    assert_eq!(
        DelimitedTable::parse_delimited("Name,Note\n\"Doe, Jane\",\"She said \"\"hi\"\"\"\nAnn,\n", ','),
        rows(&[
            &["Name", "Note"],
            &["Doe, Jane", "She said \"hi\""], // quoted separator, doubled quote
            &["Ann", ""],                      // an empty trailing field
        ])
    );

    // A quoted field may hold a line break…
    assert_eq!(DelimitedTable::parse_delimited("a,\"one\ntwo\"\n", ','), rows(&[&["a", "one\ntwo"]]));
    // …and a quote that is not at the start of a field is just a character.
    assert_eq!(DelimitedTable::parse_delimited("5\" pipe,x", ','), rows(&[&["5\" pipe", "x"]]));
    // After a quoted run closes, what follows is appended to the same field.
    assert_eq!(DelimitedTable::parse_delimited("\"a\"b,c", ','), rows(&[&["ab", "c"]]));
    // A last row with no trailing newline is still a row.
    assert_eq!(DelimitedTable::parse_delimited("a,b", ','), rows(&[&["a", "b"]]));
    assert!(DelimitedTable::parse_delimited("", ',').is_empty());
    // An empty quoted field with nothing else flushes no row: the field is empty and the row is.
    assert!(DelimitedTable::parse_delimited("\"\"", ',').is_empty());
    assert_eq!(DelimitedTable::parse_delimited(",", ','), rows(&[&["", ""]]));
    // An unterminated quoted field runs to the end and is still flushed.
    assert_eq!(DelimitedTable::parse_delimited("a,\"b\nc", ','), rows(&[&["a", "b\nc"]]));
}

#[test]
fn delimited_parsing_handles_windows_line_endings() {
    // Spreadsheet exports are exactly where CRLF comes from.
    assert_eq!(DelimitedTable::parse_delimited("a,b\r\nc,d\r\n", ','), rows(&[&["a", "b"], &["c", "d"]]));
    assert_eq!(DelimitedTable::parse_delimited("a,b\rc,d", ','), rows(&[&["a", "b"], &["c", "d"]]));
}

#[test]
fn delimited_parsing_uses_the_given_separator() {
    assert_eq!(
        DelimitedTable::parse_delimited("City\tPeople\nOslo\t709037\n", '\t'),
        rows(&[&["City", "People"], &["Oslo", "709037"]])
    );
    // A comma is content in a TSV.
    assert_eq!(DelimitedTable::parse_delimited("a,b\tc", '\t'), rows(&[&["a,b", "c"]]));
}

#[test]
fn delimited_parsing_walks_code_units() {
    // The Swift walks graphemes here, so a separator carrying a combining mark did not split there;
    // Kotlin and TypeScript walk units and do, and so does this port. Recorded.
    assert_eq!(DelimitedTable::parse_delimited("a,\u{0301}b", ','), rows(&[&["a", "\u{0301}b"]]));
    // A surrogate pair is never split by a BMP separator.
    assert_eq!(DelimitedTable::parse_delimited("\u{1F600},x", ','), rows(&[&["\u{1F600}", "x"]]));
}

#[test]
fn delimited_table_shares_one_builder() {
    let table = DelimitedTable::from("City,People\nOslo,709037", ',').expect("a table");
    assert_eq!(table.header, ["City", "People"]);
    assert_eq!(table.alignments, [Leading, Trailing]);
    assert_eq!(table.rows, rows(&[&["Oslo", "709037"]]));
    assert_eq!(DelimitedTable::from("", ','), None);
    // Equality is structural, so the same data compares equal.
    assert_eq!(DelimitedTable::from("a,b\n1,2", ','), DelimitedTable::from("a,b\r\n1,2\r\n", ','));
}

#[test]
fn numeric_columns_are_right_aligned_from_the_body_only() {
    // The header never votes: a numeric header over prose stays leading and a prose header over
    // numbers goes trailing.
    assert_eq!(alignments("1,2\n3,4", ','), [Trailing, Trailing]);
    assert_eq!(alignments("1\nx", ','), [Leading]);
    // A column with any non-numeric cell stays left-aligned.
    assert_eq!(alignments("A\n1\nn/a", ','), [Leading]);
    // Empty cells are ignored, not disqualifying.
    assert_eq!(alignments("A\n1\n\n2", ','), [Trailing]);
    // A column with no filled cell at all is leading, header only included.
    assert_eq!(alignments("A,B", ','), [Leading, Leading]);
    assert_eq!(alignments("A\n\n", ','), [Leading]);
    // Hex stays left, on every platform alike.
    assert_eq!(alignments("Item,Value\na,0x10", ','), [Leading, Leading]);
}

#[test]
fn alignment_cell_is_trimmed_with_ascii_space_and_tab_only() {
    // An invisible U+200B is not padding: Foundation's `.whitespaces` strips it and a Zs-based trim
    // does not, so the alignment scan trims ASCII only and every platform agrees the cell is not a
    // number. The opposite decision from the parser's blank-line rule.
    assert_eq!(alignments("Item,Value\na,\u{200B}1\u{200B}", ','), [Leading, Leading]);
    assert_eq!(alignments("Item,Value\na, 1 ", ','), [Leading, Trailing]);
    assert_eq!(alignments("Item,Value\na,\t1\t", ','), [Leading, Trailing]);
    assert_eq!(alignments("Item,Value\na,\u{00A0}1", ','), [Leading, Leading]);
    // The cells themselves are handed on untrimmed.
    assert_eq!(DelimitedTable::from("Item,Value\na, 1 ", ',').unwrap().rows, rows(&[&["a", " 1 "]]));
}

#[test]
fn alignments_may_outnumber_the_header() {
    // A wider body row votes for columns the header has not got; the renderer pads.
    let table = DelimitedTable::from("A\n1,2", ',').expect("a table");
    assert_eq!(table.header, ["A"]);
    assert_eq!(table.alignments, [Trailing, Trailing]);
    assert_eq!(table.rows, rows(&[&["1", "2"]]));
}

#[test]
fn tsv_uses_tabs() {
    let table = DelimitedTable::from("City\tPeople\nOslo\t709037", '\t').expect("a table");
    assert_eq!(table.header, ["City", "People"]);
    assert_eq!(table.alignments, [Leading, Trailing]);
}

#[test]
fn empty_csv_is_no_table() {
    // The renderer falls back to a bare code block so nothing the author wrote disappears.
    assert_eq!(DelimitedTable::from("", ','), None);
    assert_eq!(DelimitedTable::from("\"\"", ','), None);
    // A single newline is one row of one empty field — a table with an empty header cell.
    let blank = DelimitedTable::from("\n", ',').expect("a table");
    assert_eq!(blank.header, [""]);
    assert!(blank.rows.is_empty());
}
