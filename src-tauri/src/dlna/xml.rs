//! The XML a renderer speaks, read by string scanning rather than a parser
//! (the module header says why): `tag` and `chunks` carry the whole tolerance
//! for the shapes vendors emit, `unescape` and `escape_*` the entities.

/// Just past the `>` of the first opening `<name …>`.
///
/// **Matching on the literal `<name>` is what makes a parser vendor-specific.**
/// Three shapes are legal and all of them appear in the field: an attribute
/// (`<friendlyName xml:lang="en">`), a namespace prefix (`<av:CurrentTransportState>`
/// — the UPnP schema says out-arguments are unqualified and not every renderer
/// obeys), and the empty element `<friendlyName/>`. So the tag name is compared
/// as a *local* name against everything up to the first whitespace, and an empty
/// element is skipped rather than mistaken for content.
///
/// The delimiter check is what keeps `deviceType` from answering a search for
/// `device`; the old `format!("<{name}>")` got that right by accident and a
/// bare `starts_with` would not.
fn open_tag_end(xml: &str, name: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = xml[from..].find('<') {
        let lt = from + rel;
        from = lt + 1;
        let Some(gt_rel) = xml[lt + 1..].find('>') else {
            break;
        };
        let gt = lt + 1 + gt_rel;
        let inner = &xml[lt + 1..gt];
        // Closing tags, the declaration, comments and doctypes.
        if inner.starts_with(['/', '?', '!']) {
            continue;
        }
        // An empty element has no content to return; keep looking rather than
        // reporting the tag absent, in case a real one follows.
        if inner.ends_with('/') {
            continue;
        }
        let head = inner.split_whitespace().next().unwrap_or("");
        if head.rsplit(':').next().unwrap_or(head) == name {
            return Some(gt + 1);
        }
    }
    None
}

/// Where the first matching `</name>` begins, by local name.
fn close_tag_start(xml: &str, name: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = xml[from..].find("</") {
        let lt = from + rel;
        from = lt + 2;
        let Some(gt_rel) = xml[lt + 2..].find('>') else {
            break;
        };
        let inner = xml[lt + 2..lt + 2 + gt_rel].trim();
        if inner.rsplit(':').next().unwrap_or(inner) == name {
            return Some(lt);
        }
    }
    None
}

/// The text between the first `<tag>` and its `</tag>`, if any. Still raw —
/// call [`unescape`] on anything destined for a screen.
pub(crate) fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let start = open_tag_end(xml, name)?;
    let end = close_tag_start(&xml[start..], name)? + start;
    Some(xml[start..end].trim())
}

/// Every `<name …>` in the document, each as a slice running from just past its
/// opening tag to the end. The caller narrows further; this only replaces
/// `split("<name>")`, which cannot see an attribute or a prefix.
pub(crate) fn chunks<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = open_tag_end(&xml[from..], name) {
        from += rel;
        out.push(&xml[from..]);
    }
    out
}

/// XML text, decoded. Without this a television called "Living Room &amp; Kitchen"
/// is listed under its markup — the picker is the one place these strings are
/// read by a person.
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        // Bounded: an unescaped `&` in somebody's device name must not swallow
        // the rest of the string looking for a semicolon that never comes.
        let semi = after.find(';').filter(|&n| n > 0 && n <= 10);
        let decoded = semi.and_then(|n| match &after[..n] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            num => num
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse::<u32>().ok(),
                })
                .and_then(char::from_u32),
        });
        match (decoded, semi) {
            (Some(c), Some(n)) => {
                out.push(c);
                rest = &after[n + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escape for an XML **text node**. Attribute values additionally need `"`,
/// which [`escape_attr`] adds; nothing here may escape more than that, because
/// the result is escaped a second time on its way into `CurrentURIMetaData` and
/// every extra entity survives both rounds.
pub(super) fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub(super) fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_survive_attributes_prefixes_and_empty_elements() {
        // Every one of these is legal and at least one vendor emits it.
        assert_eq!(tag(r#"<friendlyName xml:lang="en">TV</friendlyName>"#, "friendlyName"), Some("TV"));
        assert_eq!(tag("<av:CurrentTransportState>PLAYING</av:CurrentTransportState>", "CurrentTransportState"), Some("PLAYING"));
        assert_eq!(tag("<x>a</x><Sink>b</Sink>", "Sink"), Some("b"));
        // An empty element carries nothing; the next real one wins.
        assert_eq!(tag("<UDN/><UDN>uuid:1</UDN>", "UDN"), Some("uuid:1"));
        assert_eq!(tag("<UDN/>", "UDN"), None);
        // The delimiter check: a longer name must not answer a shorter search.
        assert_eq!(tag("<deviceType>x</deviceType>", "device"), None);
        assert_eq!(tag("<modelNumber>1</modelNumber>", "modelName"), None);
        // Declarations, comments and doctypes are not elements.
        assert_eq!(tag(r#"<?xml version="1.0"?><!-- c --><name>n</name>"#, "name"), Some("n"));
        // A prefixed close tag against an unprefixed open one, and vice versa.
        assert_eq!(tag("<name>n</u:name>", "name"), Some("n"));
    }

    #[test]
    fn chunks_are_one_per_opening_tag() {
        let xml = "<service><a>1</a></service><service ><a>2</a></service>";
        let c = chunks(xml, "service");
        assert_eq!(c.len(), 2);
        assert_eq!(tag(c[0], "a"), Some("1"));
        assert_eq!(tag(c[1], "a"), Some("2"));
        assert!(chunks("<serviceList></serviceList>", "service").is_empty());
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(unescape("Living Room &amp; Kitchen"), "Living Room & Kitchen");
        assert_eq!(unescape("&lt;a&gt; &quot;b&quot; &apos;c&apos;"), "<a> \"b\" 'c'");
        assert_eq!(unescape("&#65;&#x42;"), "AB");
        assert_eq!(unescape("plain"), "plain");
        // An unescaped ampersand is left alone rather than eating the rest.
        assert_eq!(unescape("R&D and more"), "R&D and more");
        assert_eq!(unescape("&notanentity;"), "&notanentity;");
    }
}
