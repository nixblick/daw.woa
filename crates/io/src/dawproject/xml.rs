//! A small element tree on top of `quick-xml`: parse a file into [`Node`]s (with hard limits
//! against hostile input) and write a tree back with stable attribute order.
//!
//! The reader never resolves external entities or DTDs (quick-xml does not implement them),
//! so a DAWproject file cannot make the importer read other files or fetch URLs.

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};

use super::DawError;

/// Deepest element nesting accepted when reading.
pub const MAX_DEPTH: usize = 64;
/// Most elements accepted when reading.
pub const MAX_NODES: usize = 1_000_000;

/// One element with its attributes (in file order) and child elements. Text content is not
/// used by DAWproject and is dropped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    /// Element name.
    pub name: String,
    /// Attributes in order.
    pub attrs: Vec<(String, String)>,
    /// Child elements in order.
    pub children: Vec<Node>,
}

impl Node {
    /// An element without attributes and children.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    /// Adds an attribute (builder style).
    #[must_use]
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.attrs.push((key.to_owned(), value.into()));
        self
    }

    /// Adds an attribute only when `value` is `Some`.
    #[must_use]
    pub fn with_opt(self, key: &str, value: Option<String>) -> Self {
        match value {
            Some(v) => self.with(key, v),
            None => self,
        }
    }

    /// Adds a child (builder style).
    #[must_use]
    pub fn child(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }

    /// Value of an attribute.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Attribute as a finite number.
    pub fn num(&self, key: &str) -> Option<f64> {
        self.attr(key)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite())
    }

    /// Attribute as a boolean (`true`/`false`/`1`/`0`).
    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.attr(key)?.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }

    /// First child with this name.
    pub fn first(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// All children with this name.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Number of elements in the subtree, itself included.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }
}

fn xml_err(e: impl std::fmt::Display) -> DawError {
    DawError::Xml(e.to_string())
}

fn element(e: &BytesStart<'_>) -> Result<Node, DawError> {
    let name = e.name().as_ref().to_owned();
    let mut node = Node::new(&name);
    for a in e.attributes() {
        let a = a.map_err(xml_err)?;
        let key = a.key.as_ref().to_owned();
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(xml_err)?
            .into_owned();
        node.attrs.push((key, value));
    }
    Ok(node)
}

/// Parses a document into its root element.
///
/// # Errors
/// Malformed XML, nesting deeper than [`MAX_DEPTH`], more than [`MAX_NODES`] elements, or a
/// document without root element.
pub fn parse(bytes: &[u8]) -> Result<Node, DawError> {
    let text = std::str::from_utf8(bytes).map_err(xml_err)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut nodes = 0usize;
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) => {
                nodes += 1;
                if stack.len() >= MAX_DEPTH {
                    return Err(DawError::Xml(format!(
                        "Verschachtelung tiefer als {MAX_DEPTH}"
                    )));
                }
                if nodes > MAX_NODES {
                    return Err(DawError::Xml("zu viele Elemente".into()));
                }
                stack.push(element(&e)?);
            }
            Event::Empty(e) => {
                nodes += 1;
                if nodes > MAX_NODES {
                    return Err(DawError::Xml("zu viele Elemente".into()));
                }
                let node = element(&e)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::End(_) => {
                let Some(node) = stack.pop() else {
                    return Err(DawError::Xml("unerwartetes Ende-Tag".into()));
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(DawError::Xml("Datei bricht mitten im Element ab".into()));
    }
    root.ok_or_else(|| DawError::Xml("kein Wurzelelement".into()))
}

/// Text des Elements `Title` in `metadata.xml`, falls vorhanden.
pub fn metadata_title(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader::from_str(text);
    let mut in_title = false;
    let mut found = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) if e.name().as_ref() == "Title" => in_title = true,
            Event::Text(t) if in_title => found.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if in_title => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    found.push(c);
                } else {
                    match &*r {
                        "amp" => found.push('&'),
                        "lt" => found.push('<'),
                        "gt" => found.push('>'),
                        "quot" => found.push('"'),
                        "apos" => found.push('\''),
                        _ => {}
                    }
                }
            }
            Event::End(e) if e.name().as_ref() == "Title" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    let found = found.trim().to_owned();
    (!found.is_empty()).then_some(found)
}

/// `metadata.xml` mit dem Projektnamen als Titel.
pub fn write_metadata(title: &str) -> Vec<u8> {
    let esc = quick_xml::escape::escape(title);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MetaData>\n  <Title>{esc}</Title>\n</MetaData>\n"
    )
    .into_bytes()
}

fn write_node(w: &mut Writer<Vec<u8>>, n: &Node) -> Result<(), DawError> {
    let mut start = BytesStart::new(n.name.as_str());
    for (k, v) in &n.attrs {
        start.push_attribute((k.as_str(), v.as_str()));
    }
    if n.children.is_empty() {
        w.write_event(Event::Empty(start)).map_err(xml_err)?;
    } else {
        w.write_event(Event::Start(start)).map_err(xml_err)?;
        for c in &n.children {
            write_node(w, c)?;
        }
        w.write_event(Event::End(BytesEnd::new(n.name.as_str())))
            .map_err(xml_err)?;
    }
    Ok(())
}

/// Serialises a tree as UTF-8 XML with two-space indentation.
///
/// # Errors
/// Only if the in-memory writer fails, which it does not in practice.
pub fn write(root: &Node) -> Result<Vec<u8>, DawError> {
    let mut w = Writer::new_with_indent(Vec::new(), b' ', 2);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(xml_err)?;
    write_node(&mut w, root)?;
    let mut out = w.into_inner();
    out.push(b'\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back_with_escaping() {
        let tree = Node::new("A")
            .with("name", "a \"b\" <c> & d")
            .child(Node::new("B").with("x", "1"))
            .child(Node::new("C").child(Node::new("D")));
        let bytes = write(&tree).expect("write");
        let back = parse(&bytes).expect("parse");
        assert_eq!(back, tree);
    }

    #[test]
    fn metadata_title_round_trip() {
        let bytes = write_metadata("Sendung <1> & Co");
        assert_eq!(metadata_title(&bytes).as_deref(), Some("Sendung <1> & Co"));
        assert_eq!(metadata_title(b"<MetaData/>"), None);
    }

    #[test]
    fn rejects_deep_and_broken_input() {
        let deep = "<a>".repeat(MAX_DEPTH + 2);
        assert!(parse(deep.as_bytes()).is_err());
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(b"<a>").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(&[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn external_entities_are_not_followed() {
        let evil = br#"<?xml version="1.0"?><!DOCTYPE a [<!ENTITY x SYSTEM "file:///etc/passwd">]><a v="&x;"/>"#;
        // Either an error (unknown entity) or a tree without the file content.
        if let Ok(n) = parse(evil) {
            assert!(!n.attr("v").unwrap_or("").contains("root:"));
        }
    }
}
