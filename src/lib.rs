// #![allow(dead_code)]

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::{Captures, Regex};

static PLURAL_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(\w+)\s*,\s*plural\s*,(?:\s*offset:(\d+))?").unwrap());
static ORDINAL_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(\w+)\s*,\s*selectordinal\s*,").unwrap());
static SELECT_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(\w+)\s*,\s*select\s*,").unwrap());

static KV_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*=?(\w+)\s*").unwrap());
static WHITESPACES_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

const OTHER: &str = "other";

#[derive(Debug)]
pub struct MessageFormat {
    pattern: Option<String>,
    initial_literals: Vec<String>,
    parsed_pattern: Option<Vec<Block>>,
}

impl MessageFormat {
    pub fn new(pattern: String) -> Self {
        Self {
            pattern: Some(pattern),
            initial_literals: Default::default(),
            parsed_pattern: Default::default(),
        }
    }

    pub fn format(&mut self, named_parameters: Option<HashMap<String, String>>) -> String {
        self.format_impl(false, named_parameters)
    }

    pub fn format_ignoring_pound(
        &mut self,
        named_parameters: Option<HashMap<String, String>>,
    ) -> String {
        self.format_impl(true, named_parameters)
    }

    fn format_impl(
        &mut self,
        ignore_pound: bool,
        named_parameters: Option<HashMap<String, String>>,
    ) -> String {
        self.init();
        todo!()
    }

    fn init(&mut self) {
        if let Some(pattern) = self.pattern.take() {
            self.initial_literals = Default::default();
            let pattern = self.insert_placeholders(pattern);

            self.parsed_pattern = Some(self.parse_block(pattern));
        }
    }

    fn insert_placeholders(&mut self, pattern: String) -> String {
        static DOUBLE_APOSTROPHE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"''").unwrap());
        static LITERAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"'([{}#].*?)'").unwrap());

        let pattern = DOUBLE_APOSTROPHE_RE
            .replace_all(&pattern, |_caps: &Captures| self.build_placeholder("'"));
        let pattern =
            LITERAL_RE.replace_all(&pattern, |caps: &Captures| self.build_placeholder(&caps[1]));

        pattern.into_owned()
    }

    fn build_placeholder(&mut self, text: &str) -> String {
        const LITERAL_PLACEHOLDER: char = '\u{FDDF}';

        let idx = self.initial_literals.len();
        self.initial_literals.push(text.to_owned());
        format!("_{LITERAL_PLACEHOLDER}{idx}_")
    }

    fn parse_block(&mut self, pattern: String) -> Vec<Block> {
        let mut result = Vec::new();
        let parts = self.extract_parts(&pattern);
        for part in parts {
            let block = match part.typ {
                ElementType::String => Block::String(part.value),
                ElementType::Block => {
                    let block_type = self.parse_block_type(&part.value);
                    match block_type {
                        BlockType::Select => Block::Select(self.parse_select_block(&part.value)),
                        BlockType::Plural => Block::Plural(self.parse_plural_block(&part.value)),
                        BlockType::Ordinal => Block::Ordinal(self.parse_ordinal_block(&part.value)),
                        BlockType::Simple => Block::Simple(part.value),
                        _ => {
                            panic!("unknown block type for pattern {}", part.value);
                        }
                    }
                }
            };
            result.push(block);
        }
        result
    }

    fn extract_parts(&mut self, pattern: &str) -> Vec<ElementTypeAndVal> {
        static BRACES_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[{}]").unwrap());

        let mut prev_pos = 0;
        let mut brace_stack: Vec<char> = Vec::new();
        let mut results: Vec<ElementTypeAndVal> = Vec::new();

        for m in BRACES_RE.find_iter(&pattern) {
            let pos = m.start();
            if m.as_str() == "}" {
                if let Some(brace) = brace_stack.pop() {
                    assert_eq!(brace, '{', "No matching {{ for }}");
                } else {
                    panic!("No matching }} for {{");
                }
                if brace_stack.is_empty() {
                    // end of block
                    let part = ElementTypeAndVal::new(ElementType::Block, &pattern[prev_pos..pos]);
                    results.push(part);
                    prev_pos = next_char_index(&pattern[pos..]);
                }
            } else {
                if brace_stack.is_empty() {
                    let substr = &pattern[prev_pos..pos];
                    if !substr.is_empty() {
                        results.push(ElementTypeAndVal::new(ElementType::String, substr));
                    }
                    prev_pos = next_char_index(&pattern[pos..]);
                }
                brace_stack.push('{');
            }
        }

        assert!(
            brace_stack.is_empty(),
            "There are mismatched {{ or }} in the pattern"
        );

        let substr = &pattern[prev_pos..];
        if !substr.is_empty() {
            results.push(ElementTypeAndVal::new(ElementType::String, substr));
        }

        results
    }

    fn parse_block_type(&self, value: &str) -> BlockType {
        static SIMPLE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\w").unwrap());

        if PLURAL_BLOCK_RE.is_match(value) {
            BlockType::Plural
        } else if ORDINAL_BLOCK_RE.is_match(value) {
            BlockType::Ordinal
        } else if SELECT_BLOCK_RE.is_match(value) {
            BlockType::Select
        } else if SIMPLE_RE.is_match(value) {
            BlockType::Simple
        } else {
            BlockType::Unknown
        }
    }

    fn parse_select_block(&mut self, pattern: &str) -> HashMap<String, Vec<Block>> {
        let mut argument_name = None;
        let pattern = SELECT_BLOCK_RE.replace(pattern, |caps: &Captures| {
            // string, name
            argument_name = Some(caps[1].to_owned());
            ""
        });

        let mut result = HashMap::new();
        result.insert(
            "argumentName".to_owned(),
            vec![Block::String(argument_name.expect("logic error"))],
        );

        let parts = self.extract_parts(&pattern);

        // looking for (key block)+ sequence
        let mut pos = 0;
        while pos < parts.len() {
            let part = &parts[pos];
            let key = &part.value;

            pos += 1;
            assert!(pos < parts.len(), "missing or invalid select value element");
            let part = &parts[pos];

            let value = match part.typ {
                ElementType::Block => self.parse_block(part.value.clone()),
                ElementType::String => panic!("expected block type"),
            };

            let key = WHITESPACES_RE.replace_all(key, "");
            result.insert(key.into_owned(), value);

            pos += 1;
        }

        assert!(
            result.contains_key(OTHER),
            "missing other key in select statement"
        );

        result
    }

    fn parse_plural_block(&mut self, pattern: &str) -> HashMap<String, Vec<Block>> {
        let mut argument_name = None;
        let mut argument_offset = 0;
        let pattern = PLURAL_BLOCK_RE.replace(pattern, |caps: &Captures| {
            argument_name = Some(caps[1].to_owned());
            if let Some(offset) = caps.get(2) {
                argument_offset = offset.as_str().parse().unwrap();
            }
            ""
        });

        let mut result = HashMap::new();
        result.insert(
            "argumentName".to_owned(),
            vec![Block::String(argument_name.unwrap())],
        );
        result.insert(
            "argumentOffset".to_owned(),
            vec![Block::String(argument_offset.to_string())],
        );

        let parts = self.extract_parts(&pattern);

        // looking for (key block)+ sequence
        let mut pos = 0;
        while pos < parts.len() {
            let part = &parts[pos];
            let key = &part.value;

            pos += 1;
            assert!(pos < parts.len(), "missing or invalid plural element");
            let part = &parts[pos];

            let value = match part.typ {
                ElementType::Block => self.parse_block(part.value.clone()),
                ElementType::String => panic!("expected block type"),
            };

            let key = KV_RE.replace_all(key, |caps: &Captures| caps[1].to_owned());
            result.insert(key.into_owned(), value);

            pos += 1;
        }

        assert!(
            result.contains_key(OTHER),
            "missing other key in plural statement"
        );

        result
    }

    fn parse_ordinal_block(&mut self, pattern: &str) -> HashMap<String, Vec<Block>> {
        let mut argument_name = None;
        let pattern = ORDINAL_BLOCK_RE.replace(pattern, |caps: &Captures| {
            argument_name = Some(caps[1].to_owned());
            ""
        });

        let mut result = HashMap::new();
        result.insert(
            "argumentName".to_string(),
            vec![Block::String(argument_name.unwrap())],
        );
        result.insert(
            "argumentOffset".to_string(),
            vec![Block::String("0".to_owned())],
        );

        let parts = self.extract_parts(&pattern);

        // looking for (key block)+ sequence
        let mut pos = 0;
        while pos < parts.len() {
            let part = &parts[pos];
            let key = &part.value;

            pos += 1;
            assert!(
                pos < parts.len(),
                "missing or invalid ordinal value element"
            );
            let part = &parts[pos];

            let value = match part.typ {
                ElementType::Block => self.parse_block(part.value.clone()),
                ElementType::String => panic!("expected block type"),
            };

            let key = KV_RE.replace_all(key, |caps: &Captures| caps[1].to_owned());
            result.insert(key.into_owned(), value);

            pos += 1;
        }

        assert!(
            result.contains_key(OTHER),
            "missing other key in ordinal statement"
        );

        result
    }
}

fn next_char_index(s: &str) -> usize {
    s.char_indices()
        .next()
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

#[derive(Debug)]
enum Block {
    Select(HashMap<String, Vec<Block>>),
    Plural(HashMap<String, Vec<Block>>),
    Ordinal(HashMap<String, Vec<Block>>),
    String(String),
    Simple(String),
}

#[derive(Debug)]
enum BlockType {
    Plural,
    Ordinal,
    Select,
    Simple,
    Unknown,
}

#[derive(Debug, Clone)]
struct ElementTypeAndVal {
    typ: ElementType,
    value: String,
}
impl ElementTypeAndVal {
    fn new(typ: ElementType, value: impl Into<String>) -> Self {
        Self {
            typ,
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone)]
enum ElementType {
    String,
    Block,
}
