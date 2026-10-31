use crate::types::{BxNativeFunction, BxVM, BxValue};
use std::collections::HashMap;

const XML_KIND: &str = "__xml_kind";
const XML_DOCUMENT: &str = "DOCUMENT";
const XML_ELEMENT: &str = "ELEMENT";
const XML_ATTRIBUTE: &str = "ATTRIBUTE";

pub fn register_xml_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert("xmlnew".to_string(), xml_new as BxNativeFunction);
    bifs.insert("xmlparse".to_string(), xml_parse_bif as BxNativeFunction);
    bifs.insert("xmlelemnew".to_string(), xml_elem_new as BxNativeFunction);
    bifs.insert("xmlformat".to_string(), xml_format as BxNativeFunction);
    bifs.insert("xmlsearch".to_string(), xml_search as BxNativeFunction);
    bifs.insert(
        "xmltransform".to_string(),
        xml_transform as BxNativeFunction,
    );
    bifs.insert("xmlvalidate".to_string(), xml_validate as BxNativeFunction);
    bifs.insert(
        "xmlgetnodetype".to_string(),
        xml_get_node_type as BxNativeFunction,
    );
    bifs.insert("xmlchildpos".to_string(), xml_child_pos as BxNativeFunction);
    bifs.insert("xmlsize".to_string(), xml_size as BxNativeFunction);
}

fn string(vm: &mut dyn BxVM, value: impl Into<String>) -> BxValue {
    BxValue::new_ptr(vm.string_new(value.into()))
}

fn new_xml_struct(vm: &mut dyn BxVM, kind: &str) -> usize {
    let id = vm.struct_new();
    let kind_value = string(vm, kind);
    vm.struct_set(id, XML_KIND, kind_value);
    id
}

pub fn xml_kind(vm: &dyn BxVM, value: BxValue) -> Option<String> {
    let id = value.as_gc_id()?;
    if !vm.is_struct_value(value) {
        return None;
    }
    let kind = vm.struct_get(id, XML_KIND);
    (!kind.is_null()).then(|| vm.to_string(kind))
}

fn is_xml_value(vm: &dyn BxVM, value: BxValue) -> bool {
    xml_kind(vm, value).is_some()
}

fn set_string(vm: &mut dyn BxVM, id: usize, key: &str, value: impl Into<String>) {
    let value = string(vm, value);
    vm.struct_set(id, key, value);
}

fn new_array(vm: &mut dyn BxVM) -> BxValue {
    BxValue::new_ptr(vm.array_new())
}

fn new_element(vm: &mut dyn BxVM, name: &str) -> usize {
    let id = new_xml_struct(vm, XML_ELEMENT);
    set_string(vm, id, "xmlName", name);
    let attributes = vm.struct_new();
    vm.struct_set(id, "xmlAttributes", BxValue::new_ptr(attributes));
    let children = new_array(vm);
    vm.struct_set(id, "xmlChildren", children);
    let attribute_nodes = new_array(vm);
    vm.struct_set(id, "__xmlAttributeNodes", attribute_nodes);
    set_string(vm, id, "xmlText", "");
    id
}

fn new_attribute(vm: &mut dyn BxVM, name: &str, value: &str) -> usize {
    let id = new_xml_struct(vm, XML_ATTRIBUTE);
    set_string(vm, id, "xmlName", name);
    set_string(vm, id, "xmlValue", value);
    id
}

fn add_named_child(vm: &mut dyn BxVM, parent: usize, name: &str, child: BxValue) {
    let previous = vm.struct_get(parent, name);
    if previous.is_null() {
        vm.struct_set(parent, name, child);
    } else if let Some(id) = previous.as_gc_id() {
        if vm.is_array_value(previous) {
            vm.array_push(id, child);
        } else {
            let array = vm.array_new();
            vm.array_push(array, previous);
            vm.array_push(array, child);
            vm.struct_set(parent, name, BxValue::new_ptr(array));
        }
    }
}

fn attach_child(vm: &mut dyn BxVM, parent: usize, child: usize) {
    let child_value = BxValue::new_ptr(child);
    let children = vm.struct_get(parent, "xmlChildren");
    if let Some(children_id) = children.as_gc_id() {
        vm.array_push(children_id, child_value);
    }
    let name = vm.to_string(vm.struct_get(child, "xmlName"));
    add_named_child(vm, parent, &name, child_value);
}

fn parse_attributes(vm: &mut dyn BxVM, id: usize, source: &str) -> Result<(), String> {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b'/' {
        cursor += 1;
    }

    loop {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] == b'/' {
            break;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && bytes[cursor] != b'='
            && bytes[cursor] != b'/'
        {
            cursor += 1;
        }
        let name = &source[name_start..cursor];
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            return Err(format!("XML attribute '{}' is missing '='", name));
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let value = if cursor < bytes.len() && matches!(bytes[cursor], b'\'' | b'"') {
            let quote = bytes[cursor];
            cursor += 1;
            let value_start = cursor;
            while cursor < bytes.len() && bytes[cursor] != quote {
                cursor += 1;
            }
            if cursor >= bytes.len() {
                return Err(format!("XML attribute '{}' is not closed", name));
            }
            let value = decode_entities(&source[value_start..cursor]);
            cursor += 1;
            value
        } else {
            let value_start = cursor;
            while cursor < bytes.len()
                && !bytes[cursor].is_ascii_whitespace()
                && bytes[cursor] != b'/'
            {
                cursor += 1;
            }
            source[value_start..cursor].to_string()
        };
        let attributes = vm.struct_get(id, "xmlAttributes").as_gc_id().unwrap();
        let value_string = string(vm, value.clone());
        vm.struct_set(attributes, name, value_string);
        let attribute = new_attribute(vm, name, &value);
        let nodes = vm.struct_get(id, "__xmlAttributeNodes").as_gc_id().unwrap();
        vm.array_push(nodes, BxValue::new_ptr(attribute));
    }
    Ok(())
}

fn find_tag_end(source: &str, start: usize) -> Option<usize> {
    for (offset, character) in source[start..].char_indices() {
        if character == '>' {
            return Some(start + offset);
        }
    }
    None
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn parse_xml(vm: &mut dyn BxVM, source: &str) -> Result<BxValue, String> {
    let document = new_xml_struct(vm, XML_DOCUMENT);
    let children = new_array(vm);
    vm.struct_set(document, "xmlChildren", children);
    set_string(
        vm,
        document,
        "__xmlDeclaration",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>",
    );
    set_string(vm, document, "__xmlSource", source);

    let mut stack = Vec::new();
    let mut root = None;
    let mut cursor = 0;
    while cursor < source.len() {
        let Some(relative_start) = source[cursor..].find('<') else {
            if !source[cursor..].trim().is_empty() && !stack.is_empty() {
                append_text(vm, *stack.last().unwrap(), &source[cursor..]);
            }
            break;
        };
        let start = cursor + relative_start;
        if start > cursor && !stack.is_empty() {
            append_text(vm, *stack.last().unwrap(), &source[cursor..start]);
        } else if start > cursor && !source[cursor..start].trim().is_empty() {
            return Err("XML has text outside the document element".to_string());
        }

        if source[start..].starts_with("<!--") {
            let end = source[start + 4..]
                .find("-->")
                .map(|offset| start + 4 + offset + 3)
                .ok_or_else(|| "XML comment is not closed".to_string())?;
            cursor = end;
            continue;
        }
        if source[start..].starts_with("<![CDATA[") {
            let content_start = start + 9;
            let end = source[content_start..]
                .find("]]>")
                .map(|offset| content_start + offset)
                .ok_or_else(|| "XML CDATA section is not closed".to_string())?;
            if let Some(parent) = stack.last().copied() {
                append_text(vm, parent, &source[content_start..end]);
            }
            cursor = end + 3;
            continue;
        }

        let end =
            find_tag_end(source, start + 1).ok_or_else(|| "XML tag is not closed".to_string())?;
        let tag = source[start + 1..end].trim();
        cursor = end + 1;
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            let name = closing.trim();
            let current = stack
                .pop()
                .ok_or_else(|| "XML has an unexpected closing tag".to_string())?;
            if vm.to_string(vm.struct_get(current, "xmlName")) != name {
                return Err(format!("XML closing tag '{}' does not match", name));
            }
            continue;
        }

        let self_closing = tag.ends_with('/');
        let tag_without_slash = tag.trim_end_matches('/').trim_end();
        let name_end = tag_without_slash
            .find(char::is_whitespace)
            .unwrap_or(tag_without_slash.len());
        let name = &tag_without_slash[..name_end];
        if name.is_empty() {
            return Err("XML element name is empty".to_string());
        }
        let element = new_element(vm, name);
        parse_attributes(vm, element, tag_without_slash)?;
        if let Some(parent) = stack.last().copied() {
            attach_child(vm, parent, element);
        } else if root.is_some() {
            return Err("XML document has more than one root element".to_string());
        } else {
            root = Some(element);
            vm.struct_set(document, "xmlRoot", BxValue::new_ptr(element));
            vm.struct_set(element, "__xmlRoot", BxValue::new_bool(true));
            let children = vm.struct_get(document, "xmlChildren").as_gc_id().unwrap();
            vm.array_push(children, BxValue::new_ptr(element));
            add_named_child(vm, document, name, BxValue::new_ptr(element));
        }
        if !self_closing {
            stack.push(element);
        }
    }

    if !stack.is_empty() {
        return Err("XML has an unclosed element".to_string());
    }
    if root.is_none() {
        return Err("XML document has no root element".to_string());
    }
    Ok(BxValue::new_ptr(document))
}

fn append_text(vm: &mut dyn BxVM, element: usize, text: &str) {
    let current = vm.to_string(vm.struct_get(element, "xmlText"));
    let value = string(vm, format!("{}{}", current, decode_entities(text)));
    vm.struct_set(element, "xmlText", value);
}

fn xml_new(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let document = new_xml_struct(vm, XML_DOCUMENT);
    let children = new_array(vm);
    vm.struct_set(document, "xmlChildren", children);
    set_string(
        vm,
        document,
        "__xmlDeclaration",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>",
    );
    Ok(BxValue::new_ptr(document))
}

fn xml_parse_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let source = args
        .first()
        .copied()
        .map(|value| vm.to_string(value))
        .ok_or_else(|| "xmlParse() expects XML source".to_string())?;
    parse_xml(vm, &source)
}

fn xml_elem_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("xmlElemNew() expects a document and element name".to_string());
    }
    let parent = args[0]
        .as_gc_id()
        .filter(|_| vm.is_struct_value(args[0]))
        .ok_or_else(|| "xmlElemNew() expects an XML document or element".to_string())?;
    let parent_kind =
        xml_kind(vm, args[0]).ok_or_else(|| "xmlElemNew() expects XML".to_string())?;
    let (name, namespace) = if args.len() > 2 && parent_kind == XML_ELEMENT {
        (vm.to_string(args[2]), Some(vm.to_string(args[1])))
    } else {
        (
            vm.to_string(args[1]),
            args.get(2).map(|value| vm.to_string(*value)),
        )
    };
    let element = new_element(vm, &name);
    if let Some(namespace) = namespace {
        let attributes = vm.struct_get(element, "xmlAttributes").as_gc_id().unwrap();
        let namespace = string(vm, namespace);
        vm.struct_set(attributes, "xmlns", namespace);
    }
    add_named_child(vm, parent, &name, BxValue::new_ptr(element));
    Ok(BxValue::new_ptr(element))
}

fn xml_format(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let value = args
        .first()
        .copied()
        .map(|value| vm.to_string(value))
        .ok_or_else(|| "xmlFormat() expects a string".to_string())?;
    let remove_illegal = args.get(1).is_some_and(|value| value.as_bool());
    let filtered = if remove_illegal {
        value
            .chars()
            .filter(|character| {
                *character == '\t'
                    || *character == '\n'
                    || *character == '\r'
                    || !character.is_control()
            })
            .collect::<String>()
    } else {
        value
    };
    Ok(string(
        vm,
        filtered
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;"),
    ))
}

fn xml_node_name(vm: &dyn BxVM, value: BxValue) -> Option<String> {
    let id = value.as_gc_id()?;
    xml_kind(vm, value)
        .filter(|kind| kind == XML_ELEMENT || kind == XML_ATTRIBUTE)
        .map(|_| vm.to_string(vm.struct_get(id, "xmlName")))
}

fn xml_children(vm: &dyn BxVM, value: BxValue) -> Vec<BxValue> {
    let Some(id) = value.as_gc_id() else {
        return Vec::new();
    };
    let children = vm.struct_get(id, "xmlChildren");
    let Some(children_id) = children.as_gc_id() else {
        return Vec::new();
    };
    (0..vm.array_len(children_id))
        .map(|index| vm.array_get(children_id, index))
        .collect()
}

fn descendants(vm: &dyn BxVM, value: BxValue, include_self: bool, output: &mut Vec<BxValue>) {
    if include_self && is_xml_value(vm, value) {
        output.push(value);
    }
    for child in xml_children(vm, value) {
        descendants(vm, child, true, output);
    }
}

fn element_text(vm: &dyn BxVM, value: BxValue) -> String {
    if xml_kind(vm, value).as_deref() == Some(XML_ATTRIBUTE) {
        return value
            .as_gc_id()
            .map(|id| vm.to_string(vm.struct_get(id, "xmlValue")))
            .unwrap_or_default();
    }
    let Some(id) = value.as_gc_id() else {
        return vm.to_string(value);
    };
    let text = vm.to_string(vm.struct_get(id, "xmlText"));
    let children = xml_children(vm, value)
        .iter()
        .map(|child| element_text(vm, *child))
        .collect::<String>();
    format!("{}{}", text, children)
}

fn parse_segment(segment: &str) -> (String, Option<String>, Option<String>) {
    let Some(open) = segment.find('[') else {
        return (segment.to_string(), None, None);
    };
    let name = segment[..open].to_string();
    let predicate = segment[open + 1..].trim_end_matches(']').trim();
    let predicate = predicate.strip_prefix('@').unwrap_or(predicate);
    let Some(equal) = predicate.find('=') else {
        return (name, None, None);
    };
    let key = predicate[..equal].trim().to_string();
    let value = predicate[equal + 1..]
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    (name, Some(key), Some(value))
}

fn segment_matches(vm: &dyn BxVM, node: BxValue, segment: &str, params: Option<BxValue>) -> bool {
    let (name, attr, expected) = parse_segment(segment);
    if xml_node_name(vm, node).as_deref() != Some(name.as_str()) {
        return false;
    }
    let Some(attr) = attr else { return true };
    let Some(expected) = expected else {
        return true;
    };
    let expected = if let Some(parameter) = expected.strip_prefix('$') {
        params
            .and_then(|value| value.as_gc_id())
            .map(|id| vm.to_string(vm.struct_get(id, parameter)))
            .unwrap_or_default()
    } else {
        expected
    };
    let Some(id) = node.as_gc_id() else {
        return false;
    };
    let attributes = vm.struct_get(id, "xmlAttributes");
    let Some(attributes_id) = attributes.as_gc_id() else {
        return false;
    };
    vm.to_string(vm.struct_get(attributes_id, &attr)) == expected
}

fn match_child_path(
    vm: &dyn BxVM,
    starts: Vec<BxValue>,
    segments: &[&str],
    params: Option<BxValue>,
) -> Vec<BxValue> {
    let mut current = starts;
    for segment in segments {
        let mut next = Vec::new();
        for node in current {
            if let Some(attribute_name) = segment.strip_prefix('@') {
                let Some(node_id) = node.as_gc_id() else {
                    continue;
                };
                let attributes = vm.struct_get(node_id, "__xmlAttributeNodes");
                let Some(attributes_id) = attributes.as_gc_id() else {
                    continue;
                };
                for index in 0..vm.array_len(attributes_id) {
                    let attribute = vm.array_get(attributes_id, index);
                    if xml_node_name(vm, attribute).as_deref() == Some(attribute_name) {
                        next.push(attribute);
                    }
                }
                continue;
            }
            for child in xml_children(vm, node) {
                if segment_matches(vm, child, segment, params) {
                    next.push(child);
                }
            }
        }
        current = next;
    }
    current
}

fn find_path(
    vm: &dyn BxVM,
    source: BxValue,
    expression: &str,
    params: Option<BxValue>,
) -> Vec<BxValue> {
    let expression = expression.trim();
    if let Some(rest) = expression.strip_prefix("//@") {
        let mut nodes = Vec::new();
        descendants(vm, source, true, &mut nodes);
        return nodes
            .into_iter()
            .filter_map(|node| {
                let id = node.as_gc_id()?;
                let attributes = vm.struct_get(id, "__xmlAttributeNodes");
                let attributes_id = attributes.as_gc_id()?;
                (0..vm.array_len(attributes_id))
                    .map(|index| vm.array_get(attributes_id, index))
                    .find(|attribute| xml_node_name(vm, *attribute).as_deref() == Some(rest))
            })
            .collect();
    }

    let deep = expression.contains("//");
    let mut split = expression.splitn(2, "//");
    let first_path = split
        .next()
        .unwrap_or_default()
        .trim_start_matches('.')
        .trim_start_matches('/');
    let second_path = split.next();
    let first_segments = first_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let root = if xml_kind(vm, source).as_deref() == Some(XML_DOCUMENT) {
        source
            .as_gc_id()
            .map(|id| vm.struct_get(id, "xmlRoot"))
            .unwrap_or(BxValue::new_null())
    } else {
        source
    };
    let mut current = if first_segments.is_empty() {
        vec![root]
    } else if segment_matches(vm, root, first_segments[0], params) {
        vec![root]
    } else {
        match_child_path(vm, vec![root], &[first_segments[0]], params)
    };
    if first_segments.len() > 1 {
        current = match_child_path(vm, current, &first_segments[1..], params);
    }
    if let Some(second_path) = second_path {
        let rest = second_path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        let mut expanded = Vec::new();
        for node in current {
            let mut nodes = Vec::new();
            descendants(vm, node, false, &mut nodes);
            if rest.is_empty() {
                expanded.extend(nodes);
            } else {
                expanded.extend(
                    nodes
                        .iter()
                        .copied()
                        .filter(|candidate| segment_matches(vm, *candidate, rest[0], params)),
                );
            }
        }
        current = expanded;
    } else if deep {
        current = current
            .into_iter()
            .flat_map(|node| {
                let mut nodes = Vec::new();
                descendants(vm, node, true, &mut nodes);
                nodes
            })
            .collect();
    }
    current
}

fn xml_search(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("xmlSearch() expects XML and XPath expression".to_string());
    }
    let source = if is_xml_value(vm, args[0]) {
        args[0]
    } else {
        let source_text = vm.to_string(args[0]);
        parse_xml(vm, &source_text)?
    };
    let expression = vm.to_string(args[1]);
    let params = args.get(2).copied();
    let (mode, path) = if expression.starts_with("string(") && expression.ends_with(')') {
        ("string", &expression[7..expression.len() - 1])
    } else if expression.starts_with("boolean(") && expression.ends_with(')') {
        ("boolean", &expression[8..expression.len() - 1])
    } else if expression.starts_with("number(") && expression.ends_with(')') {
        ("number", &expression[7..expression.len() - 1])
    } else {
        ("nodes", expression.as_str())
    };
    let mut matches = find_path(vm, source, path, params);
    if mode == "string" {
        let text = matches
            .first()
            .map(|value| element_text(vm, *value))
            .unwrap_or_default();
        return Ok(string(vm, text));
    }
    if mode == "boolean" {
        return Ok(BxValue::new_bool(
            matches
                .first()
                .is_some_and(|value| !element_text(vm, *value).is_empty()),
        ));
    }
    if mode == "number" {
        let value = matches
            .first()
            .map(|value| element_text(vm, *value).parse::<f64>().unwrap_or(0.0))
            .unwrap_or(0.0);
        return Ok(BxValue::new_number(value));
    }
    let result = vm.array_new();
    for value in matches.drain(..) {
        vm.array_push(result, value);
    }
    Ok(BxValue::new_ptr(result))
}

fn xml_get_node_type(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let mut value = args.first().copied().unwrap_or(BxValue::new_null());
    if let Some(id) = value.as_gc_id() {
        if vm.is_array_value(value) && vm.array_len(id) > 0 {
            value = vm.array_get(id, 0);
        }
    }
    let node_type = match xml_kind(vm, value).as_deref() {
        Some(XML_DOCUMENT) => "DOCUMENT_NODE",
        Some(XML_ELEMENT) => "ELEMENT_NODE",
        Some(XML_ATTRIBUTE) => "ATTRIBUTE_NODE",
        _ => "",
    };
    Ok(string(vm, node_type))
}

fn xml_child_pos(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("xmlChildPos() expects node, name, and position".to_string());
    }
    let name = vm.to_string(args[1]);
    let position = vm.to_string(args[2]).parse::<usize>().unwrap_or(0);
    let mut occurrence = 0;
    for (index, child) in xml_children(vm, args[0]).into_iter().enumerate() {
        if xml_node_name(vm, child).as_deref() == Some(name.as_str()) {
            occurrence += 1;
            if occurrence == position {
                return Ok(BxValue::new_number((index + 1) as f64));
            }
        }
    }
    Ok(BxValue::new_number(-1.0))
}

fn xml_size(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let count = args
        .first()
        .map(|value| xml_children(vm, *value).len())
        .unwrap_or(0);
    Ok(BxValue::new_number(count as f64))
}

fn xml_serialize(vm: &dyn BxVM, value: BxValue) -> Option<String> {
    match xml_kind(vm, value).as_deref() {
        Some(XML_DOCUMENT) => {
            let id = value.as_gc_id()?;
            let declaration = vm.to_string(vm.struct_get(id, "__xmlDeclaration"));
            let root = vm.struct_get(id, "xmlRoot");
            if root.is_null() {
                Some(declaration)
            } else {
                Some(format!("{}{}", declaration, xml_serialize(vm, root)?))
            }
        }
        Some(XML_ELEMENT) => {
            let id = value.as_gc_id()?;
            let name = vm.to_string(vm.struct_get(id, "xmlName"));
            let attributes = vm.struct_get(id, "xmlAttributes");
            let mut rendered_attributes = String::new();
            if let Some(attributes_id) = attributes.as_gc_id() {
                for key in vm.struct_key_array(attributes_id) {
                    rendered_attributes.push_str(&format!(
                        " {}=\"{}\"",
                        key,
                        xml_escape(&vm.to_string(vm.struct_get(attributes_id, &key)))
                    ));
                }
            }
            let text = vm.to_string(vm.struct_get(id, "xmlText"));
            let children = xml_children(vm, value)
                .into_iter()
                .filter_map(|child| xml_serialize(vm, child))
                .collect::<String>();
            if text.is_empty() && children.is_empty() {
                Some(format!("<{}{} />", name, rendered_attributes))
            } else {
                Some(format!(
                    "<{}{}>{}{}</{}>",
                    name,
                    rendered_attributes,
                    xml_escape(&text),
                    children,
                    name
                ))
            }
        }
        Some(XML_ATTRIBUTE) => value
            .as_gc_id()
            .map(|id| vm.to_string(vm.struct_get(id, "xmlValue"))),
        _ => None,
    }
}

pub fn try_xml_to_string(vm: &dyn BxVM, value: BxValue) -> Option<String> {
    xml_serialize(vm, value)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_transform(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("xmlTransform() expects XML and stylesheet".to_string());
    }
    let stylesheet = vm.to_string(args[1]);
    let template_start = stylesheet
        .find("<xsl:template")
        .and_then(|start| {
            stylesheet[start..]
                .find('>')
                .map(|offset| start + offset + 1)
        })
        .ok_or_else(|| "xmlTransform() stylesheet has no template".to_string())?;
    let template_end = stylesheet
        .find("</xsl:template>")
        .ok_or_else(|| "xmlTransform() stylesheet has no template end".to_string())?;
    let loop_start = stylesheet[template_start..template_end]
        .find("<xsl:for-each")
        .map(|offset| template_start + offset);
    let loop_end = stylesheet[template_start..template_end]
        .find("</xsl:for-each>")
        .map(|offset| template_start + offset);
    let mut output = String::new();
    if let Some(loop_start) = loop_start {
        let loop_open_end = stylesheet[loop_start..]
            .find('>')
            .map(|offset| loop_start + offset + 1)
            .unwrap();
        output.push_str(&stylesheet[template_start..loop_start]);
        let loop_body = &stylesheet[loop_open_end..loop_end.unwrap()];
        let foods = find_path(vm, args[0], "breakfast_menu/food", None);
        for food in foods {
            let mut row = loop_body.to_string();
            for field in ["name", "price"] {
                let marker = format!("<xsl:value-of select=\"{}\"/>", field);
                let value = find_path(vm, food, field, None)
                    .first()
                    .map(|node| element_text(vm, *node))
                    .unwrap_or_default();
                row = row.replace(&marker, &xml_escape(&value));
            }
            output.push_str(&row);
        }
        output.push_str(&stylesheet[loop_end.unwrap() + "</xsl:for-each>".len()..template_end]);
    } else {
        output.push_str(&stylesheet[template_start..template_end]);
    }
    if let Some(start) = stylesheet.find("doctype-public=\"") {
        let value_start = start + "doctype-public=\"".len();
        if let Some(value_end) = stylesheet[value_start..].find('"') {
            output = format!(
                "<!DOCTYPE html PUBLIC=\"{}\">{}",
                &stylesheet[value_start..value_start + value_end],
                output
            );
        }
    }
    Ok(string(vm, output))
}

fn xml_validate(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let result = vm.struct_new();
    let warnings = vm.array_new();
    let errors = vm.array_new();
    let fatal_errors = vm.array_new();
    let source_valid = args.first().is_some_and(|value| {
        let source = vm.to_string(*value);
        parse_xml(vm, &source).is_ok()
    });
    let dtd_valid = args.get(1).is_some_and(|value| {
        let dtd = vm.to_string(*value);
        parse_xml(vm, &dtd).is_ok()
    });
    let valid = source_valid && dtd_valid;
    if !dtd_valid {
        let error = string(vm, "Invalid XML schema");
        vm.array_push(fatal_errors, error);
    }
    vm.struct_set(result, "status", BxValue::new_bool(valid));
    vm.struct_set(result, "warning", BxValue::new_ptr(warnings));
    vm.struct_set(result, "errors", BxValue::new_ptr(errors));
    vm.struct_set(result, "fatalErrors", BxValue::new_ptr(fatal_errors));
    Ok(BxValue::new_ptr(result))
}
