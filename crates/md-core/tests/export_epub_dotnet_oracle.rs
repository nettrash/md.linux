//! md.win's EPUB regexes and string rules, replayed against the hand-written matchers that replace
//! them here (md-core has no regex crate). Every expected value below was computed by .NET 10's
//! `System.Text.RegularExpressions` running md.win's patterns copied verbatim from `EpubExport.cs`
//! (`ScriptElement`, `VoidElement`) and `EpubMetadata.cs` (`LanguageTag`, `W3CDate` + `IsW3CDate`,
//! `SeriesIndexPattern`, `Base64Payload`, `Scheme`, `Reference`), `string.Equals(…,
//! OrdinalIgnoreCase)` for the front-matter keys, and `DateTimeOffset.ToString` for the timestamp.
//!
//! They are a selection from a seeded differential run (seed 20261027, about 346 000 inputs: random
//! token soups, mutated and exhaustive-range dates, structured `data:` URIs, homoglyph keys,
//! instants across `DateTimeOffset`'s whole range) that found no divergence, plus hand-picked
//! cases for the whitespace class inside a tag. The selection keeps each matcher pinned to the
//! engine it replaces: a future edit that changes one answer fails here with the .NET value beside
//! it.

use std::time::{Duration, UNIX_EPOCH};

use md_core::export::epub_export;
use md_core::export::epub_metadata::{self, CoverReference};
use md_core::markdown::MetadataField;

const XHTML: [(&str, &str); 65] = [
    ("<br\u{2028}>\u{2028}", "<br/>\u{2028}"),
    ("<img>\u{2029}", "<img/>\u{2029}"),
    ("\u{B}<hr>\u{3000}", "\u{B}<hr/>\u{3000}"),
    ("&bull;\u{1680}", "&#8226;\u{1680}"),
    ("\u{A0}<hr>", "\u{A0}<hr/>"),
    ("\u{2007}<img>", "\u{2007}<img/>"),
    ("<hr\u{85}/>", "<hr/>"),
    ("<br\u{B}/>", "<br/>"),
    ("\u{C}&bull;", "\u{C}&#8226;"),
    ("<br\r/>", "<br/>"),
    ("<brt>\t", "<brt/>\t"),
    ("\n&bull;", "\n&#8226;"),
    ("&bull;\u{200B}", "&#8226;\u{200B}"),
    ("<img>\u{180E}", "<img/>\u{180E}"),
    ("\"&bull;", "\"&#8226;"),
    ("&bull;alt=\"a > b\"", "&#8226;alt=\"a > b\""),
    ("<br>x'", "<br/>x'"),
    ("<br<BR>", "<br<BR/>"),
    ("<brxt>", "<brxt/>"),
    ("<br<SCRIPT>", "<br<SCRIPT/>"),
    ("<br</script>", "<br</script/>"),
    ("<hr<script type=\"module\">", "<hr<script type=\"module\"/>"),
    ("&bull;", "&#8226;"),
    ("\r&bull;", "\r&#8226;"),
    ("\u{1F642}<br>", "\u{1F642}<br/>"),
    ("<imgt>\u{41F}\u{440}\u{438}", "<imgt/>\u{41F}\u{440}\u{438}"),
    ("  / /<hr<\u{85}<BR><SCRIPT>\u{2028}'", "  / /<hr<\u{85}<BR/><SCRIPT>\u{2028}'"),
    ("<img>", "<img/>"),
    ("<hr>", "<hr/>"),
    ("<br>", "<br/>"),
    ("<br", "<br"),
    ("<script", "<script"),
    ("\"", "\""),
    ("\u{3000}", "\u{3000}"),
    (
        "\u{B}\u{2028}\u{2028}t><script>\u{2029}<hra=\"b\"\u{200B}</p>\r<hr\u{B}<script",
        "\u{B}\u{2028}\u{2028}t><script>\u{2029}<hra=\"b\"\u{200B}</p/>\r<hr\u{B}<script",
    ),
    ("</scrip\u{1F642}  />\u{A0}\u{C}<hrmg&bull;&bull;<<  ", "</scrip\u{1F642}  />\u{A0}\u{C}<hrmg&#8226;&#8226;<<  "),
    ("brx<brx<BR></scripr  <bra=\"b\"<", "brx<brx<BR/></scripr  <bra=\"b\"<"),
    ("\u{C}<script type=\"module\">mg\t\u{180E}\u{1F642}x\u{85}alt=\"a > b\"</script>'t>\u{A0}'", "\u{C}'t>\u{A0}'"),
    (
        "  \u{41F}\u{440}\u{438}/><\u{2029}\"</SCRIPT>\"<script>\u{2007}</script>\u{41F}\u{440}\u{438}\n  ",
        "  \u{41F}\u{440}\u{438}/><\u{2029}\"</SCRIPT>\"\u{41F}\u{440}\u{438}\n  ",
    ),
    ("<script\u{3000}\t\u{3000}<img&bull;t>&bullbrx", "<script\u{3000}\t\u{3000}<img&#8226;t/>&bullbrx"),
    ("<br\u{3000}>", "<br/>"),
    ("<br\u{200B}>", "<br\u{200B}/>"),
    ("<br\u{180E}>", "<br\u{180E}/>"),
    ("<br\u{85}>", "<br/>"),
    ("<br\u{2028}/>", "<br/>"),
    ("<br\u{A0}\u{2029} />", "<br/>"),
    ("<br / >", "<br //>"),
    ("<img alt=\"a\u{3000}>\"\u{3000}>", "<img alt=\"a\u{3000}>\"/>"),
    ("<img alt=\"unclosed>", "<img alt=\"unclosed>"),
    ("<img alt=\"x\" title=\"y\"\t\n/>", "<img alt=\"x\" title=\"y\"/>"),
    ("<hr class=x>", "<hr class=x/>"),
    ("<hrx>", "<hrx/>"),
    ("<html>", "<html>"),
    ("<b>", "<b>"),
    ("<script>a</script><script>b", "<script>b"),
    ("<script x=\">\">y</script>z", "z"),
    ("<script\n>q</script>", ""),
    ("<SCRIPT>a</SCRIPT>", "<SCRIPT>a</SCRIPT>"),
    ("<script></script><br>", "<br/>"),
    ("x<script>a<br>b</script>y<br>", "xy<br/>"),
    (
        "<br>\u{41F}\u{440}\u{438}\u{432}\u{435}\u{442}<img src=\"\u{43A}.png\">",
        "<br/>\u{41F}\u{440}\u{438}\u{432}\u{435}\u{442}<img src=\"\u{43A}.png\"/>",
    ),
    ("<img src=\"a\"b\">", "<img src=\"a\"b\">"),
    ("<img \"\">", "<img \"\"/>"),
    ("<br\"", "<br\""),
    (">\"<br>", ">\"<br/>"),
];
const DATES: [(&str, bool); 36] = [
    ("9321", true),
    ("1229", true),
    ("5858", true),
    ("0291-12", true),
    ("0856", true),
    ("8948", true),
    ("3192", true),
    ("0558", true),
    ("1200", true),
    ("7586", true),
    ("7224", true),
    ("1003", true),
    ("3792", true),
    ("1212", true),
    ("9666", true),
    ("7365", true),
    ("0882", true),
    ("9893", true),
    ("+t", false),
    ("18", false),
    ("\n-\n", false),
    ("\n+T", false),
    ("\u{662}T", false),
    ("\nT", false),
    ("\u{662}2", false),
    ("\u{662}Z", false),
    ("94", false),
    ("\n-", false),
    ("17", false),
    ("\n ", false),
    ("zZ", false),
    ("- ", false),
    ("37", false),
    ("9\u{FF12}", false),
    ("-2", false),
    ("z ", false),
];
const LANGUAGES: [(&str, bool); 20] = [
    ("nn", true),
    ("znh", true),
    ("REh", true),
    ("zeE", true),
    ("nhn-9", true),
    ("BRR", true),
    ("hx", true),
    ("zxz", true),
    ("zB-1", true),
    ("Bee", true),
    ("\u{131}1", false),
    ("1\u{E9}", false),
    ("--B", false),
    ("-e\u{E9}", false),
    ("-\u{131}", false),
    ("--\u{663}", false),
    ("-z", false),
    ("-B\u{663}", false),
    ("R\u{17F}", false),
    ("\u{FF25}n", false),
];
const SERIES_INDEXES: [(&str, bool); 16] = [
    ("1001025", true),
    ("25251", true),
    ("01250", true),
    ("0250.1", true),
    ("000251", true),
    ("10250", true),
    ("2525", true),
    ("101", true),
    ("+25", false),
    ("a1a", false),
    ("\u{663}.+", false),
    ("-+\u{663}", false),
    ("aa", false),
    ("10.", false),
    ("-a1", false),
    ("1\n-", false),
];
const REFERENCES: [(&str, &str); 50] = [
    ("data:;base64,xrI=", "inline:c6b2"),
    ("data:;base64,i+==", "inline:8b"),
    ("data:;base64,s7LP", "inline:b3b2cf"),
    ("data:;base64,aPU=", "inline:68f5"),
    ("data:;base64,8jE=", "inline:f231"),
    ("data:;base64,Eg==", "inline:12"),
    ("data:x\u{17F};base64,+YJx", "inline:f98271"),
    ("data:;base64,oixL", "inline:a22c4b"),
    ("data:;base64,KA==", "inline:28"),
    ("data:;base64,I0g=", "inline:2348"),
    ("https://!QUJD#", "https:https://!QUJD#"),
    ("https://\u{435}AAAA", "https:https://\u{435}AAAA"),
    ("https://.", "https:https://."),
    ("HTTPS://.#", "https:HTTPS://.#"),
    ("https://\u{131}DATA:", "https:https://\u{131}DATA:"),
    ("HTTPS://!\\QUJD,", "https:HTTPS://!\\QUJD,"),
    ("HTTPS:// AB==", "https:HTTPS:// AB=="),
    ("HTTPS://AAAA ", "https:HTTPS://AAAA "),
    ("HTTPS://\\\u{17F}://", "https:HTTPS://\\\u{17F}://"),
    ("https://+/==/=", "https:https://+/==/="),
    ("..\\", "local:..\\"),
    (",  ", "local:,  "),
    (" \u{435}", "local: \u{435}"),
    ("!\u{17F}", "local:!\u{17F}"),
    ("\u{435}\u{131}", "local:\u{435}\u{131}"),
    (",#=", "local:,#="),
    (" \u{131}", "local: \u{131}"),
    (" \\", "local: \\"),
    ("\nx!", "local:\nx!"),
    ("..=", "local:..="),
    ("://x", "none"),
    ("c:#", "none"),
    ("/,,\u{17F}", "none"),
    ("#C: ", "none"),
    ("#=", "none"),
    ("C:#\\", "none"),
    ("#\\C:", "none"),
    ("/=,", "none"),
    ("#/\u{17F}", "none"),
    ("C:x\u{435}", "none"),
    ("C:", "none"),
    ("C:\u{435}", "none"),
    ("c:.", "none"),
    ("#\u{131},", "none"),
    ("\\=\u{435}", "none"),
    ("//..", "none"),
    ("/.==", "none"),
    ("C:\u{131}\u{131}", "none"),
    ("\\\u{131}", "none"),
    ("/=\n", "none"),
];
const KEYS: [(&str, &str, bool); 18] = [
    ("seRi\u{435}\u{17F}_IndeX", "series_index", false),
    ("sEri\u{435}S_index", "series_index", false),
    ("seri\u{435}s_\u{130}ndex", "series_index", false),
    ("de\u{17F}cRiptIon", "description", false),
    ("T\u{130}tl\u{435}", "title", false),
    ("\u{17F}er\u{130}esx", "series", false),
    ("seRiEs_ind\u{435}x", "series_index", false),
    ("seri\u{435}s_\u{130}ndexx", "series_index", false),
    ("de\u{17F}cRiption", "description", false),
    ("s\u{435}r\u{130}e\u{17F}", "series", false),
    ("sEr\u{130}\u{435}S", "series", false),
    ("lAnguag\u{435}", "language", false),
    ("laNGUagE", "language", true),
    ("dEscrIptIon", "description", true),
    ("SerieS_inDex", "series_index", true),
    ("LaNguage", "language", true),
    ("aUtHOr", "author", true),
    ("dEscrIptiON", "description", true),
];
const MODIFIED: [(i64, &str); 16] = [
    (239129607, "1970-01-03T18:25:29Z"),
    (167353279537198, "7273-03-19T06:05:37Z"),
    (49780902118658, "3547-06-30T20:21:58Z"),
    (12939807776755, "2380-01-18T07:02:56Z"),
    (114242634, "1970-01-02T07:44:02Z"),
    (-51009604, "1969-12-31T09:49:50Z"),
    (62115661368222, "3938-05-15T06:22:48Z"),
    (31612241622917, "2971-10-02T10:13:42Z"),
    (117953126231959, "5707-10-15T05:17:11Z"),
    (98639382537223, "5095-10-04T12:28:57Z"),
    (-3933681461346, "1845-05-07T06:42:18Z"),
    (4538844603056, "2113-10-30T22:10:03Z"),
    (1472920132392, "2016-09-03T16:28:52Z"),
    (-3907198255876, "1846-03-09T19:09:04Z"),
    (175273763292500, "7524-03-16T12:28:12Z"),
    (191985961, "1970-01-03T05:19:45Z"),
];

#[test]
fn the_xhtml_fixer_agrees_with_the_dotnet_regexes() {
    for (input, expected) in XHTML {
        assert_eq!(expected, epub_export::xhtml_body(input), "{input:?}");
    }
}

#[test]
fn the_date_check_agrees_with_the_dotnet_regex_and_ranges() {
    for (input, expected) in DATES {
        assert_eq!(expected, epub_metadata::is_w3c_date(input), "{input:?}");
    }
}

#[test]
fn the_language_tag_agrees_with_the_dotnet_regex() {
    // Through the public rule: a matching tag is kept, anything else becomes `en`.
    for (input, matches) in LANGUAGES {
        let language = epub_metadata::from_front_matter(&[MetadataField::new("language", input)]).language;
        assert_eq!(matches, language == input, "{input:?} became {language:?}");
    }
}

#[test]
fn the_series_index_agrees_with_the_dotnet_regex() {
    for (input, matches) in SERIES_INDEXES {
        let metadata = epub_metadata::from_front_matter(&[
            MetadataField::new("series", "S"),
            MetadataField::new("series_index", input),
        ]);
        assert_eq!(matches, metadata.series_index.as_deref() == Some(input), "{input:?}");
    }
}

#[test]
fn the_cover_reference_agrees_with_dotnet() {
    fn described(reference: Option<CoverReference>) -> String {
        match reference {
            None => "none".to_string(),
            Some(CoverReference::Inline(data)) => {
                format!("inline:{}", data.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
            }
            Some(CoverReference::Https(url)) => format!("https:{url}"),
            Some(CoverReference::Local(path)) => format!("local:{path}"),
        }
    }
    for (input, expected) in REFERENCES {
        assert_eq!(expected, described(epub_metadata::reference(Some(input))), "{input:?}");
    }
}

#[test]
fn the_key_match_agrees_with_ordinal_ignore_case() {
    // .NET's ordinal casing maps U+0131 and U+017F to themselves (and U+212A, U+0130 never fold to
    // ASCII), so an ASCII fold is exact against an ASCII key: these homoglyphs match nothing.
    for (candidate, key, expected) in KEYS {
        let found = epub_metadata::field_value(&[MetadataField::new(candidate, "v")], key).is_some();
        assert_eq!(expected, found, "{candidate:?} against {key:?}");
    }
}

#[test]
fn the_timestamp_agrees_with_datetimeoffset() {
    for (milliseconds, expected) in MODIFIED {
        let when = if milliseconds >= 0 {
            UNIX_EPOCH + Duration::from_millis(milliseconds.unsigned_abs())
        } else {
            UNIX_EPOCH - Duration::from_millis(milliseconds.unsigned_abs())
        };
        assert_eq!(expected, epub_export::modified(when), "{milliseconds}");
    }
}
