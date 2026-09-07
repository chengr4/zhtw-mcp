// Language scoping for HTML tags embedded in Markdown.
//
// An author who wraps a run in <span lang="en"> or <div lang="en"> has already
// said that the run is not Chinese. Honoring that keeps the linter off text it
// has no business judging, and it guesses nothing: only a tag that carries an
// explicit lang attribute counts, which is exactly what the author declared.
//
// This is a tag matcher, not an HTML parser. It reads the tag name and the lang
// attribute and ignores everything else, because that is all the exclusion
// decision needs. Nesting is tracked with a stack of open elements so that a
// zh-TW span inside an English block is scanned again, and so that a same-name
// element nested inside a scope does not close it early.

use super::excluded::ByteRange;

// The Chinese macrolanguage: "zh" itself plus the ISO 639-3 varieties that
// belong to it. A run marked with any of these is Chinese prose, so it stays
// scanned; that includes zh-CN and zh-Hans, which are precisely the input this
// linter exists to rewrite.
const CHINESE_PRIMARY_SUBTAGS: &[&str] = &[
    "zh", "cdo", "cjy", "cmn", "cnp", "cpx", "csp", "czh", "czo", "gan", "hak", "hsn", "lzh",
    "mnp", "nan", "wuu", "yue",
];

// Elements that cannot contain anything, so their lang attribute scopes no text
// and they must never be pushed onto the open-element stack. Without this a
// stray <br> would swallow every following close tag's match.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

// Elements whose content is text rather than markup. A tag written inside one
// is a string, and a Markdown file explaining HTML is exactly where a "</div>"
// or an unclosed <span lang="en"> turns up inside a script. Reading those as
// markup would pop an element the page never closed, or open a scope nothing
// closes and silence the rest of the document, so everything up to the matching
// closer is skipped.
const RAW_TEXT_ELEMENTS: &[&str] = &[
    "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
];

// Block-level starters that close an open p. HTML gives p an optional end tag,
// so an author who wrote two paragraphs without closing the first meant two
// paragraphs, and it is the second one's lang that applies to its own text.
const CLOSES_PARAGRAPH: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "search",
    "section",
    "table",
    "ul",
];

// Elements that hold text rather than structure. The implicit close below looks
// through these, and through the three block elements HTML's own list item
// algorithm names, and stops at anything more structural.
const INLINE_ELEMENTS: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "big", "cite", "code", "data", "del", "dfn", "em", "font", "i",
    "ins", "kbd", "label", "mark", "nobr", "output", "picture", "q", "ruby", "s", "samp", "small",
    "span", "strike", "strong", "sub", "sup", "time", "tt", "u", "var",
];

// Elements that belong to a table and are ended by the next one in the same
// table. What stops a search for them is a nested table, not a block element in
// between, which is the "in table scope" rule HTML states for them.
const TABLE_PARTS: &[&str] = &["td", "th", "tr", "tbody", "thead", "tfoot"];

/// Whether the search for an element that "incoming" implicitly ends may look
/// past an open "element".
///
/// The two families differ in HTML and differ here.
///
/// A cell or a row is ended by the next one anywhere inside the same table, so
/// only a nested table stops that search. Those three names are HTML's "in
/// table scope" list.
///
/// Everything else follows the li start tag algorithm, which walks the stack
/// and stops at the first element in HTML's "special" category that is not an
/// address, a div or a p. Special is nearly every structural element, section
/// and article and blockquote among them, so the walk here looks past inline
/// content and those three names and stops at the rest. That the exemption is
/// exactly three elements looks arbitrary and is not: it is what makes
/// "<ul><li>a<div><li>b" two items and "<ul><li>a<section><li>b" one item
/// holding a nested one, in a browser and here.
fn transparent_to_implicit_close(element: &str, incoming: &str) -> bool {
    if TABLE_PARTS.contains(&incoming) {
        return !matches!(element, "table" | "template" | "html");
    }
    INLINE_ELEMENTS.contains(&element) || matches!(element, "address" | "div" | "p")
}

/// How deep the open-element stack is allowed to get.
///
/// Every search in here is bounded by the stack, and the stack was bounded
/// only by the document, so markup that nests thousands deep cost time in the
/// square of its size on an entry point that accepts 256 KiB of it. Past this
/// depth a tag is counted rather than tracked, so its lang scopes nothing and
/// the text under it stays scanned: the direction that lints too much rather
/// than the one that silently skips prose. Browsers cap nesting for the same
/// reason and in the same range.
const MAX_DEPTH: usize = 512;

/// Elements a following sibling can close without an end tag of their own.
///
/// The left-hand side of closed_implicitly_by, as a set. When none of these is
/// open there is nothing for the implicit-close walk to find, which is what
/// lets it skip the walk rather than scan a stack that cannot answer.
const IMPLICITLY_CLOSABLE: &[&str] = &[
    "p", "li", "dt", "dd", "td", "th", "tr", "thead", "tbody", "option", "optgroup", "rt", "rp",
    "colgroup",
];

/// Whether an open element is implicitly closed when "incoming" starts.
///
/// The end tag is optional for these, and a browser closes them on the next
/// sibling rather than waiting for a closer that never comes. Without this the
/// stack keeps an English paragraph open across the Chinese one that follows
/// it, and the run the author marked as Chinese goes unscanned.
fn closed_implicitly_by(open: &str, incoming: &str) -> bool {
    match open {
        "p" => CLOSES_PARAGRAPH.contains(&incoming),
        "li" => incoming == "li",
        "dt" | "dd" => matches!(incoming, "dt" | "dd"),
        "td" | "th" => matches!(incoming, "td" | "th" | "tr"),
        "tr" => matches!(incoming, "tr" | "tbody" | "tfoot" | "thead"),
        "thead" | "tbody" => matches!(incoming, "tbody" | "tfoot"),
        "option" => matches!(incoming, "option" | "optgroup"),
        "optgroup" => incoming == "optgroup",
        "rt" | "rp" => matches!(incoming, "rt" | "rp"),

        // Only a col stays inside a colgroup, so anything else ends it. Without
        // this a lang on a column group went on scoping the rest of the table.
        "colgroup" => incoming != "col",
        _ => false,
    }
}

/// Whether a BCP 47 language tag names a Chinese variety.
///
/// Only the primary subtag is inspected, so zh, zh-TW, zh-Hant-TW and ZH_CN
/// all answer true. An empty tag answers false: HTML gives lang="" the meaning
/// "language unknown", which is not a declaration that the run is not Chinese.
fn is_chinese_lang(tag: &str) -> bool {
    let primary = tag.trim().split(['-', '_']).next().unwrap_or_default();
    CHINESE_PRIMARY_SUBTAGS
        .iter()
        .any(|known| primary.eq_ignore_ascii_case(known))
}

/// Whether a declared lang value means "scan nothing under this element".
///
/// True only for a non-empty tag that names something other than Chinese. An
/// empty value leaves the run unmarked rather than marking it foreign.
///
/// The browser extension resolves the lang it read off the page through this
/// too, so the two halves cannot disagree about what counts as foreign.
pub(crate) fn excludes(tag: &str) -> bool {
    !tag.trim().is_empty() && !is_chinese_lang(tag)
}

/// One open element, with what the lang it declared means if it declared one.
struct OpenElement {
    /// ASCII-lowercased tag name, for case-insensitive close matching.
    name: String,
    /// Whether this element's own lang marks its text as foreign. None when it
    /// declared no lang at all, which is what lets an inner declaration of the
    /// empty string cancel an outer one rather than inherit it.
    foreign: Option<bool>,
}

/// Accumulates lang-scoped exclusion ranges from HTML chunks fed in document
/// order.
///
/// Feed every HTML run the Markdown parser reports, then call finish. The
/// ranges come back unsorted relative to the caller's other ranges, which is
/// what merge_ranges_pub already expects.
#[derive(Default)]
pub(crate) struct LangScopes {
    open: Vec<OpenElement>,
    /// Indices into "open" of the elements that declared a lang, innermost
    /// last. What the current text inherits is the innermost declaration, and
    /// reading it off the back of this is what keeps a stack of elements that
    /// declared nothing from being rescanned for every tag that follows.
    declared: Vec<usize>,
    /// How many entries in "open" a sibling could close implicitly. Zero means
    /// the walk in close_implied_by has nothing to find, and deeply nested
    /// markup is otherwise a stack walk per start tag.
    closable: usize,
    /// How many elements are open past MAX_DEPTH. While this is non-zero the
    /// tracker only balances tags against it, so the stack it walks stays
    /// bounded. Well-formed markup closes what it opened, which is what brings
    /// this back to zero and resumes tracking where it left off.
    suppressed: usize,
    /// Start of the exclusion currently being accumulated, if any.
    pending: Option<usize>,
    /// Whether the innermost open element is one whose content is text rather
    /// than markup. Held across feed calls because pulldown-cmark reports an
    /// inline script as a tag, its text, and a tag, in three separate events.
    /// The element itself is on the stack, so its name is not repeated here.
    in_raw_text: bool,
    ranges: Vec<ByteRange>,
}

impl LangScopes {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed one HTML run. "base" is the byte offset of "chunk" within the
    /// document, so the ranges that come out are document offsets.
    ///
    /// Chunks must arrive in document order; the pulldown-cmark offset
    /// iterator yields Html and InlineHtml events that way.
    pub(crate) fn feed(&mut self, chunk: &str, base: usize) {
        let bytes = chunk.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'<' {
                i += 1;
                continue;
            }

            // A comment, a declaration, or a lone angle bracket yields no tag
            // but still reports where it ends, so its contents are skipped.
            let (tag, next) = scan_token(chunk, i);
            if let Some(tag) = tag {
                self.apply(tag, base + i, base + next);
            }
            i = next;
        }
    }

    /// Close out the accumulated ranges. An element left open at "text_end"
    /// scopes to there, the way an unclosed element in HTML is closed by the
    /// end of what contains it.
    pub(crate) fn finish(mut self, text_end: usize) -> Vec<ByteRange> {
        self.close_pending(text_end);
        self.ranges
    }

    /// Fold in one tag, whose bytes span [start, end) in the document.
    fn apply(&mut self, tag: Tag<'_>, start: usize, end: usize) {
        // Past the depth cap nothing is tracked, only balanced, so that the
        // stack every search below walks cannot grow with the document.
        if self.suppressed > 0 {
            if tag.closing {
                self.suppressed -= 1;
            } else if !tag.self_closing && !VOID_ELEMENTS.contains(&tag.name.as_str()) {
                self.suppressed += 1;
            }
            return;
        }

        // Inside a raw-text element only its own closer is markup, and that
        // element is the innermost open one. Its closer falls through to the
        // path below, which pops it and ends the scope its own lang opened.
        if self.in_raw_text {
            let closes_it = tag.closing
                && self
                    .open
                    .last()
                    .is_some_and(|innermost| innermost.name == tag.name);
            if !closes_it {
                return;
            }
            self.in_raw_text = false;
        }

        if tag.closing {
            // Pop to the innermost element of this name. An unmatched closer
            // names nothing on the stack and is ignored, which is what a
            // browser does with it too.
            if let Some(idx) = self.open.iter().rposition(|open| open.name == tag.name) {
                self.truncate_open(idx);
                self.settle(start, end);
            }
            return;
        }

        // A void or self-closed element contains no text, so its lang scopes
        // nothing and it never joins the stack.
        if tag.self_closing || VOID_ELEMENTS.contains(&tag.name.as_str()) {
            return;
        }

        // A raw-text element still scopes its own lang, so it is pushed like
        // any other; what changes is that until its closer arrives, nothing
        // inside it is markup.
        let raw = RAW_TEXT_ELEMENTS.contains(&tag.name.as_str());

        self.close_implied_by(&tag.name);
        if self.open.len() >= MAX_DEPTH {
            self.suppressed = 1;
            return;
        }
        self.push_open(OpenElement {
            name: tag.name,
            foreign: tag.lang.map(excludes),
        });
        self.settle(start, end);
        self.in_raw_text = raw;
    }

    /// Close the innermost element that "incoming" implicitly ends, along with
    /// anything open inside it.
    ///
    /// The search looks past what HTML looks past, so a span or a div between
    /// two list items does not stop the second from ending the first, and it
    /// stops where HTML stops, so a nested list or table does.
    fn close_implied_by(&mut self, incoming: &str) {
        // One start tag can end more than one element, and stopping at the
        // first left the rest open: a row starting inside a cell closed the
        // cell and kept the row holding it, so the old row went on scoping the
        // new one's text. Every pass drops an element, so this terminates.
        while self.close_one_implied_by(incoming) {}
    }

    /// Close the innermost element that "incoming" ends, if there is one, and
    /// report whether it closed anything so the caller can ask again.
    fn close_one_implied_by(&mut self, incoming: &str) -> bool {
        // Nothing open can be closed this way, so the walk below could only run
        // off the bottom of the stack. Markup nested hundreds deep makes that
        // walk the difference between linear and quadratic.
        if self.closable == 0 {
            return false;
        }
        for (idx, open) in self.open.iter().enumerate().rev() {
            if closed_implicitly_by(&open.name, incoming) {
                self.truncate_open(idx);
                return true;
            }
            if !transparent_to_implicit_close(&open.name, incoming) {
                return false;
            }
        }
        false
    }

    /// Open one element, recording what it contributes to the indexes.
    fn push_open(&mut self, element: OpenElement) {
        if element.foreign.is_some() {
            self.declared.push(self.open.len());
        }
        if IMPLICITLY_CLOSABLE.contains(&element.name.as_str()) {
            self.closable += 1;
        }
        self.open.push(element);
    }

    /// Drop every element from "idx" up, keeping the indexes in step.
    ///
    /// Each element is counted once when it opens and uncounted once when it
    /// closes, so the cost of maintaining them is linear in the tags fed
    /// rather than in the depth they reach.
    fn truncate_open(&mut self, idx: usize) {
        for open in self.open.drain(idx..) {
            if IMPLICITLY_CLOSABLE.contains(&open.name.as_str()) {
                self.closable -= 1;
            }
        }
        while self.declared.last().is_some_and(|&at| at >= idx) {
            self.declared.pop();
        }
    }

    /// Record the transition, if any, that the tag spanning [start, end)
    /// caused.
    ///
    /// The tag itself is folded into the range on both sides. That costs
    /// nothing: markdown.rs already excludes every HTML event's own bytes.
    fn settle(&mut self, start: usize, end: usize) {
        match (self.pending.is_some(), self.foreign()) {
            (false, true) => self.pending = Some(start),
            (true, false) => self.close_pending(end),
            _ => {}
        }
    }

    /// End the exclusion in progress at "end", if there is one. The one place
    /// that writes a range down, so finish and settle cannot disagree about
    /// what an empty one is.
    fn close_pending(&mut self, end: usize) {
        if let Some(start) = self.pending.take().filter(|&start| start < end) {
            self.ranges.push(ByteRange { start, end });
        }
    }

    /// Whether the innermost declared lang marks the current text as foreign.
    fn foreign(&self) -> bool {
        self.declared
            .last()
            .and_then(|&at| self.open[at].foreign)
            .unwrap_or(false)
    }
}

/// One parsed tag: everything the exclusion decision reads, and nothing else.
/// Where the tag sits is not here, because the caller is standing on it.
struct Tag<'a> {
    /// ASCII-lowercased tag name.
    name: String,
    closing: bool,
    self_closing: bool,
    /// The lang attribute value. An attribute written without a value reads as
    /// the empty string, matching HTML.
    lang: Option<&'a str>,
}

/// Read one token starting at the '<' at "start".
///
/// Returns the parsed tag, if the token is one, and the offset to continue
/// from. A comment or a declaration is not a tag but still reports where it
/// ends, so its contents are skipped; a '<' that begins neither costs one byte.
fn scan_token(chunk: &str, start: usize) -> (Option<Tag<'_>>, usize) {
    let bytes = chunk.as_bytes();
    let Some(rest) = bytes.get(start + 1..) else {
        return (None, start + 1);
    };

    if rest.starts_with(b"!--") {
        let from = start + 4;
        let end = chunk[from.min(chunk.len())..]
            .find("-->")
            .map_or(chunk.len(), |i| from + i + 3);
        return (None, end);
    }

    // A CDATA section ends at "]]>", not at the first '>'. Stopping early would
    // resume reading its contents as markup.
    if rest.starts_with(b"![CDATA[") {
        let from = start + 9;
        let end = chunk[from.min(chunk.len())..]
            .find("]]>")
            .map_or(chunk.len(), |i| from + i + 3);
        return (None, end);
    }
    if matches!(rest.first(), Some(b'!' | b'?')) {
        let end = chunk[start..]
            .find('>')
            .map_or(chunk.len(), |i| start + i + 1);
        return (None, end);
    }

    let closing = rest.first() == Some(&b'/');
    let mut i = start + 1 + usize::from(closing);
    let name_start = i;
    while bytes.get(i).is_some_and(|b| is_name_byte(*b)) {
        i += 1;
    }
    if i == name_start || !bytes[name_start].is_ascii_alphabetic() {
        return (None, start + 1);
    }
    let name = chunk[name_start..i].to_ascii_lowercase();

    let mut lang = None;
    let mut self_closing = false;
    loop {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        match bytes.get(i) {
            // An unterminated tag runs to the end of the chunk. pulldown-cmark
            // only reports complete tags, so this is defensive.
            None => break,
            Some(b'>') => {
                i += 1;
                break;
            }
            Some(b'/') => {
                self_closing = true;
                i += 1;
                continue;
            }
            _ => {}
        }
        self_closing = false;

        let attr_start = i;
        while bytes
            .get(i)
            .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/'))
        {
            i += 1;
        }
        if i == attr_start {
            // Nothing consumed and nothing matched above: step over the byte
            // rather than spin.
            i += 1;
            continue;
        }
        let attr = &chunk[attr_start..i];

        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let value = if bytes.get(i) == Some(&b'=') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            Some(read_value(chunk, &mut i))
        } else {
            None
        };

        // A repeated attribute is dropped rather than applied, so the first
        // lang written is the one a browser parses and the one that has to
        // decide the scope here.
        if attr.eq_ignore_ascii_case("lang") && lang.is_none() {
            lang = Some(value.unwrap_or(""));
        }
    }

    (
        Some(Tag {
            name,
            closing,
            self_closing,
            lang,
        }),
        i,
    )
}

/// Read an attribute value at "*i", advancing past it.
fn read_value<'a>(chunk: &'a str, i: &mut usize) -> &'a str {
    let bytes = chunk.as_bytes();
    match bytes.get(*i) {
        Some(&quote @ (b'"' | b'\'')) => {
            let value_start = *i + 1;
            let mut end = value_start;
            while bytes.get(end).is_some_and(|b| *b != quote) {
                end += 1;
            }
            *i = (end + 1).min(chunk.len());
            &chunk[value_start..end]
        }
        _ => {
            let value_start = *i;
            let mut end = value_start;
            while bytes
                .get(end)
                .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>')
            {
                end += 1;
            }
            *i = end;
            &chunk[value_start..end]
        }
    }
}

/// Bytes that may appear in a tag name. Hyphen and colon are here for custom
/// elements and for namespaced SVG or MathML names.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

#[cfg(test)]
#[path = "../../tests/unit/engine/html_lang/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/engine/html_lang/invariants.rs"]
mod invariants;
