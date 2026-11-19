//! Tiny XML tree used for Plex (`/status/sessions` has no JSON API).

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Debug, Default, Clone)]
pub struct Node {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|node| node.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |node| node.name == name)
    }
}

fn node_of(start: &BytesStart<'_>) -> Result<Node, String> {
    let mut node = Node {
        name: String::from_utf8_lossy(start.name().as_ref()).to_string(),
        ..Node::default()
    };
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|err| err.to_string())?;
        let value = attribute
            .unescape_value()
            .map_err(|err| err.to_string())?
            .to_string();
        node.attrs.insert(
            String::from_utf8_lossy(attribute.key.as_ref()).to_string(),
            value,
        );
    }
    Ok(node)
}

/// Parses a document and returns its root element.
pub fn parse(xml: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event().map_err(|err| err.to_string())? {
            Event::Start(start) => stack.push(node_of(&start)?),
            Event::Empty(start) => {
                let node = node_of(&start)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or("unbalanced XML")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    root.ok_or_else(|| "empty XML document".to_string())
}
