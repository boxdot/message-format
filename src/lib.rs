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
    parsed_pattern: Vec<Block>,
}

impl MessageFormat {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: Some(pattern.into()),
            initial_literals: Default::default(),
            parsed_pattern: Default::default(),
        }
    }

    pub fn format(&mut self) -> String {
        self.format_impl(false, None)
    }

    pub fn format_with_params(
        &mut self,
        named_parameters: impl IntoIterator<Item = (impl Into<String>, impl ToString)>,
    ) -> String {
        self.format_impl(
            false,
            Some(
                named_parameters
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.to_string()))
                    .collect(),
            ),
        )
    }

    pub fn format_ignoring_pound(
        &mut self,
        named_parameters: impl IntoIterator<Item = (impl Into<String>, impl ToString)>,
    ) -> String {
        self.format_impl(
            true,
            Some(
                named_parameters
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.to_string()))
                    .collect(),
            ),
        )
    }

    fn format_impl(
        &mut self,
        ignore_pound: bool,
        named_parameters: Option<HashMap<String, String>>,
    ) -> String {
        self.init();
        dbg!(&self);
        dbg!(&named_parameters);

        if self.parsed_pattern.is_empty() {
            return String::new();
        }

        let mut literals = self.initial_literals.clone();

        let mut message_parts = Vec::new();
        format_block(
            &self.parsed_pattern,
            named_parameters.as_ref().unwrap_or(&HashMap::new()),
            &mut literals,
            ignore_pound,
            &mut message_parts,
        );
        let mut message = message_parts.join("");

        if !ignore_pound {
            assert!(!message.contains('#'), "not all # were replaced");
        }

        while let Some(literal) = literals.pop() {
            let placeholder = placeholder(literals.len());
            message = message.replacen(&placeholder, &literal, 1);
        }

        message
    }

    fn init(&mut self) {
        if let Some(pattern) = self.pattern.take() {
            self.initial_literals = Default::default();
            let pattern = self.insert_placeholders(pattern);

            self.parsed_pattern = self.parse_block(pattern);
        }
    }

    fn insert_placeholders(&mut self, pattern: String) -> String {
        static DOUBLE_APOSTROPHE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"''").unwrap());
        static LITERAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"'([{}#].*?)'").unwrap());

        let pattern = DOUBLE_APOSTROPHE_RE.replace_all(&pattern, |_caps: &Captures| {
            Self::build_placeholder(&mut self.initial_literals, "'")
        });
        let pattern = LITERAL_RE.replace_all(&pattern, |caps: &Captures| {
            Self::build_placeholder(&mut self.initial_literals, &caps[1])
        });

        pattern.into_owned()
    }

    fn build_placeholder(literals: &mut Vec<String>, text: &str) -> String {
        let idx = literals.len();
        literals.push(text.to_owned());
        placeholder(idx)
    }

    fn parse_block(&mut self, pattern: String) -> Vec<Block> {
        let mut result = Vec::new();
        let parts = self.extract_parts(&pattern);
        dbg!(&parts);
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

        for m in BRACES_RE.find_iter(pattern) {
            let pos = m.start();
            if m.as_str() == "}" {
                if let Some(brace) = brace_stack.pop() {
                    assert_eq!(brace, '{', "No matching }} for {{");
                } else {
                    panic!("No matching {{ for }}");
                }
                if brace_stack.is_empty() {
                    // end of block
                    let part = ElementTypeAndVal::new(ElementType::Block, &pattern[prev_pos..pos]);
                    results.push(part);
                    prev_pos = pos + 1; // Note: } is single byte, so index arithmetic is ok for UTF-8
                }
            } else {
                if brace_stack.is_empty() {
                    let substr = &pattern[prev_pos..pos];
                    if !substr.is_empty() {
                        results.push(ElementTypeAndVal::new(ElementType::String, substr));
                    }
                    prev_pos = pos + 1; // Note: { is single byte, so index arithmetic is ok for UTF-8
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
                ElementType::String => panic!("assert_eqed block type"),
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
                ElementType::String => panic!("assert_eqed block type"),
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
                ElementType::String => panic!("assert_eqed block type"),
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

fn placeholder(idx: usize) -> String {
    const LITERAL_PLACEHOLDER: &str = "\u{FDDF}_";
    format!("_{LITERAL_PLACEHOLDER}{idx}_")
}

fn format_block(
    parsed_blocks: &[Block],
    named_parameters: &HashMap<String, String>,
    literals: &mut Vec<String>,
    ignore_pound: bool,
    result: &mut Vec<String>,
) {
    for current_pattern in parsed_blocks {
        match current_pattern {
            Block::String(value) => {
                result.push(value.clone());
            }
            Block::Simple(value) => {
                format_simple_placeholder(value, named_parameters, literals, result);
            }
            Block::Select(map_pattern) => {
                format_select_block(
                    map_pattern,
                    named_parameters,
                    literals,
                    ignore_pound,
                    result,
                );
            }
            Block::Plural(value) => {
                format_plural_ordinal_block(
                    value,
                    named_parameters,
                    literals,
                    |n| Some(plural_rules_select(n)),
                    ignore_pound,
                    result,
                );
            }
            Block::Ordinal(value) => {
                format_plural_ordinal_block(
                    value,
                    named_parameters,
                    literals,
                    |n| Some(ordinal_rules_select(n)),
                    ignore_pound,
                    result,
                );
            }
        }
    }
}

fn format_simple_placeholder(
    param: &str,
    named_parameters: &HashMap<String, String>,
    literals: &mut Vec<String>,
    result: &mut Vec<String>,
) {
    let Some(value) = named_parameters.get(param) else {
        result.push(format!("Undefined parameter - {param}"));
        return;
    };

    // TODO: handle int formatting

    let placeholder = placeholder(literals.len());
    literals.push(value.to_string());
    result.push(placeholder);
}

fn format_select_block(
    parsed_blocks: &HashMap<String, Vec<Block>>,
    named_parameters: &HashMap<String, String>,
    literals: &mut Vec<String>,
    ignore_pound: bool,
    result: &mut Vec<String>,
) {
    let Some(Block::String(argument_name)) =
        parsed_blocks.get("argumentName").and_then(|b| b.first())
    else {
        panic!("invalid argument name");
    };

    let Some(param) = named_parameters.get(argument_name) else {
        result.push(format!("Undefined parameter - {argument_name}"));
        return;
    };

    let Some(option) = parsed_blocks
        .get(param)
        .or_else(|| parsed_blocks.get(OTHER))
    else {
        panic!("Invalid option or missing other option for select block");
    };

    format_block(option, named_parameters, literals, ignore_pound, result);
}

fn format_plural_ordinal_block(
    parsed_blocks: &HashMap<String, Vec<Block>>,
    named_parameters: &HashMap<String, String>,
    literals: &mut Vec<String>,
    plural_selector: impl Fn(u64) -> Option<&'static str>, // TODO: add locale
    ignore_pound: bool,
    result: &mut Vec<String>,
) {
    let Some(Block::String(argument_name)) =
        parsed_blocks.get("argumentName").and_then(|b| b.first())
    else {
        panic!("invalid argument name");
    };
    let Some(Block::String(argument_offset)) =
        parsed_blocks.get("argumentOffset").and_then(|b| b.first())
    else {
        panic!("invalid argument offset");
    };

    let Some(plural_value) = named_parameters.get(argument_name) else {
        result.push(format!("Undefined parameter - {argument_name}"));
        return;
    };

    let Ok(plural_value) = plural_value.parse::<i64>() else {
        result.push(format!("Invalid parameter - {argument_name}"));
        return;
    };

    let Ok(argument_offset) = argument_offset.parse::<i64>() else {
        result.push(format!("Invalid offset - {argument_offset}"));
        return;
    };

    let diff = plural_value - argument_offset;

    let option = match parsed_blocks.get(&named_parameters[argument_name]) {
        Some(option) => option,
        None => {
            let diff: u64 = diff.abs().try_into().unwrap();
            let item = plural_selector(diff).expect("Invalid plural key");
            let Some(option) = parsed_blocks.get(item).or_else(|| parsed_blocks.get(OTHER)) else {
                panic!("Invalid option or missing other option for plural block");
            };
            option
        }
    };

    let mut plural_result = Vec::new();
    format_block(
        option,
        named_parameters,
        literals,
        ignore_pound,
        &mut plural_result,
    );
    let plural = plural_result.join("");
    if ignore_pound {
        result.push(plural);
    } else {
        // TODO: locale aware formatting
        let diff = diff.to_string();
        result.push(plural.replace('#', &diff));
    }
}

fn plural_rules_select(n: u64) -> &'static str {
    // TODO: make locale aware
    match n {
        0 => "zero",
        1 => "one",
        2 => "two",
        3..=5 => "few",
        6.. => "many",
    }
}

fn ordinal_rules_select(n: u64) -> &'static str {
    // Ordinals are not supported
    // <https://github.com/dart-lang/i18n/blob/98e7b4aea2e6ff613ec273ca29f58938d9c5b23d/pkgs/intl/lib/message_format.dart#L771>
    plural_rules_select(n)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pattern() {
        let mut fmt = MessageFormat::new("");
        assert_eq!(fmt.format(), "");
    }

    #[test]
    #[should_panic(expected = "No matching { for }")]
    fn test_missing_left_curly_brace() {
        let mut fmt = MessageFormat::new("\'\'{}}");
        fmt.format();
    }

    #[test]
    #[should_panic(expected = "There are mismatched { or } in the pattern")]
    fn test_too_many_left_curly_braces() {
        let mut fmt = MessageFormat::new("{} {");
        fmt.format();
    }

    #[test]
    fn test_simple_replacement() {
        let mut fmt = MessageFormat::new("New York in {SEASON} is nice.");
        assert_eq!(
            fmt.format_with_params([("SEASON", "the Summer")]),
            "New York in the Summer is nice."
        );
    }

    #[test]
    fn test_simple_select() {
        let mut fmt = MessageFormat::new(
            "{GENDER, select,\
            male {His}\
            female {Her}\
            other {Their}} \
            bicycle is {GENDER, select, male {blue} female {red} other {green}}.",
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "male")]),
            "His bicycle is blue."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female")]),
            "Her bicycle is red."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "other")]),
            "Their bicycle is green."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "any")]),
            "Their bicycle is green."
        );
    }

    #[test]
    fn test_simple_plural() {
        let mut fmt = MessageFormat::new(
            "I see {NUM_PEOPLE, plural, offset:1 \
            =0 {no one at all in {PLACE}.} \
            =1 {{PERSON} in {PLACE}.} \
            one {{PERSON} and one other person in {PLACE}.} \
            other {{PERSON} and # other people in {PLACE}.}}",
        );
        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", "0"), ("PLACE", "Belgrade")]),
            "I see no one at all in Belgrade."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "1"),
                ("PERSON", "Markus"),
                ("PLACE", "Berlin")
            ]),
            "I see Markus in Berlin."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", "2"), ("PERSON", "Mark"), ("PLACE", "Athens")]),
            "I see Mark and one other person in Athens."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "100"),
                ("PERSON", "Cibu"),
                ("PLACE", "the cubes")
            ]),
            "I see Cibu and 99 other people in the cubes."
        );
    }

    #[ignore = "needs decimal formatting"]
    #[test]
    fn test_select_nested_in_plural() {
        let mut fmt = MessageFormat::new(
            "{CIRCLES, plural, \
        one {{GENDER, select, \
          female {{WHO} added you to her circle} \
          other  {{WHO} added you to his circle}}} \
        other {{GENDER, select,
          female {{WHO} added you to her # circles} \
          other  {{WHO} added you to his # circles}}}}",
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female"), ("WHO", "Jelena"), ("CIRCLES", "1")]),
            "Jelena added you to her circle",
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "male"), ("WHO", "Milan"), ("CIRCLES", "1234")]),
            "Milan added you to his 1,234 circles",
        );
    }

    #[ignore = "needs decimal formatting"]
    #[test]
    fn test_plural_nested_in_select() {
        // Added offset just for testing purposes. It doesn't make sense to have it otherwise.
        let mut fmt = MessageFormat::new(
            "{GENDER, select, \
        female {{NUM_GROUPS, plural, \
          one {{WHO} added you to her group} \
          other {{WHO} added you to her # groups}}} \
        other {{NUM_GROUPS, plural, offset,1\
          one {{WHO} added you to his group} \
          other {{WHO} added you to his # groups}}}}",
        );

        assert_eq!(
            fmt.format_with_params([("GENDER", "female"), ("WHO", "Jelena"), ("NUM_GROUPS", "1")]),
            "Jelena added you to her group",
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "male"), ("WHO", "Milan"), ("NUM_GROUPS", "1234")]),
            "Milan added you to his 1,233 groups",
        );
    }

    #[test]
    fn test_literal_open_curly_brace() {
        let mut fmt =
            MessageFormat::new("Anna's house has '{0} and # in the roof' and {NUM_COWS} cows.");
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", "5")]),
            "Anna's house has {0} and # in the roof and 5 cows."
        );
    }

    #[test]
    fn test_literal_closed_curly_brace() {
        let mut fmt =
            MessageFormat::new("Anna's house has '{'0'} and # in the roof' and {NUM_COWS} cows.");
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", "5")]),
            "Anna's house has {0} and # in the roof and 5 cows."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", "8")]),
            "Anna's house has {0} and # in the roof and 8 cows."
        );
    }

    #[test]
    fn test_literal_pound_sign() {
        let mut fmt =
            MessageFormat::new("Anna's house has '{0}' and '# in the roof' and {NUM_COWS} cows.");
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", "5")]),
            "Anna's house has {0} and # in the roof and 5 cows."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", "10")]),
            "Anna's house has {0} and # in the roof and 10 cows."
        );
    }

    #[test]
    fn test_no_literals_for_single_quotes() {
        let mut fmt = MessageFormat::new("Anna's house 'has {NUM_COWS} cows'.");
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", "5")]),
            "Anna's house 'has 5 cows'."
        );
    }

    #[test]
    fn test_consecutive_single_quotes_are_replaced_with_one_single_quote() {
        let mut fmt = MessageFormat::new("Anna''s house a'{''''b'");
        assert_eq!(fmt.format(), "Anna's house a{''b");
    }

    #[test]
    fn test_test_consecutive_single_quotes_before_special_char_dont_create_literal() {
        let mut fmt = MessageFormat::new("a''{NUM_COWS}'b");
        assert_eq!(fmt.format_with_params([("NUM_COWS", "5")]), "a'5'b");
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_serbian_simple_select() {
        let mut fmt = MessageFormat::new(
            "{GENDER, select, female {Njen} other {Njegov}} bicikl je \
             {GENDER, select, female {crven} other {plav}}.",
        );

        assert_eq!(
            fmt.format_with_params([("GENDER", "male")]),
            "Njegov bicikl je plav."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female")]),
            "Njen bicikl je crven."
        );
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_serbian_simple_plural() {
        let mut fmt = MessageFormat::new(
            "Ja {NUM_PEOPLE, plural, offset,1 \
            =0 {ne vidim nikoga} \
            =1 {vidim {PERSON}} \
            one {vidim {PERSON} i jos # osobu} \
            few {vidim {PERSON} i jos # osobe} \
            many {vidim {PERSON} i jos # osoba} \
            other {vidim {PERSON} i jos # osoba}} \
          u {PLACE}.",
        );

        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", "0"), ("PLACE", "Beogradu")]),
            "Ja ne vidim nikoga u Beogradu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "1"),
                ("PERSON", "Markusa"),
                ("PLACE", "Berlinu")
            ]),
            "Ja vidim Markusa u Berlinu."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", "2"), ("PERSON", "Marka"), ("PLACE", "Atini")]),
            "Ja vidim Marka i jos 1 osobu u Atini."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "4"),
                ("PERSON", "Petra"),
                ("PLACE", "muzeju")
            ]),
            "Ja vidim Petra i jos 3 osobe u muzeju."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "100"),
                ("PERSON", "Cibua"),
                ("PLACE", "bazenu")
            ]),
            "Ja vidim Cibua i jos 99 osoba u bazenu."
        );
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_test_serbian_simple_plural_no_offset() {
        let mut fmt = MessageFormat::new(
            "Ja {NUM_PEOPLE, plural, \
            =0 {ne vidim nikoga} \
            =1 {vidim {PERSON}} \
            one {vidim {PERSON} i jos # osobu} \
            few {vidim {PERSON} i jos # osobe} \
            many {vidim {PERSON} i jos # osoba} \
            other {vidim {PERSON} i jos # osoba}} \
          u {PLACE}.",
        );

        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", "0"), ("PLACE", "Beogradu")]),
            "Ja ne vidim nikoga u Beogradu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "1"),
                ("PERSON", "Markusa"),
                ("PLACE", "Berlinu")
            ]),
            "Ja vidim Markusa u Berlinu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "21"),
                ("PERSON", "Marka"),
                ("PLACE", "Atini")
            ]),
            "Ja vidim Marka i jos 21 osobu u Atini."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "3"),
                ("PERSON", "Petra"),
                ("PLACE", "muzeju")
            ]),
            "Ja vidim Petra i jos 3 osobe u muzeju."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", "100"),
                ("PERSON", "Cibua"),
                ("PLACE", "bazenu")
            ]),
            "Ja vidim Cibua i jos 100 osoba u bazenu."
        );
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_test_serbian_select_nested_in_plural() {
        let mut fmt = MessageFormat::new(
            "{CIRCLES, plural, \
            one {{GENDER, select, \
              female {{WHO} vas je dodala u njen # kruzok} \
              other  {{WHO} vas je dodao u njegov # kruzok}}} \
            few {{GENDER, select, \
              female {{WHO} vas je dodala u njena # kruzoka} \
              other  {{WHO} vas je dodao u njegova # kruzoka}}} \
            many {{GENDER, select, \
              female {{WHO} vas je dodala u njenih # kruzoka} \
              other  {{WHO} vas je dodao u njegovih # kruzoka}}} \
            other {{GENDER, select, \
              female {{WHO} vas je dodala u njenih # kruzoka} \
              other  {{WHO} vas je dodao u njegovih # kruzoka}}}}",
        );

        assert_eq!(
            fmt.format_with_params([("GENDER", "female"), ("WHO", "Jelena"), ("CIRCLES", "21")]),
            "Jelena vas je dodala u njen 21 kruzok"
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female"), ("WHO", "Jelena"), ("CIRCLES", "3")]),
            "Jelena vas je dodala u njena 3 kruzoka"
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female"), ("WHO", "Jelena"), ("CIRCLES", "5")]),
            "Jelena vas je dodala u njenih 5 kruzoka"
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "male"), ("WHO", "Milan"), ("CIRCLES", "1235")]),
            "Milan vas je dodao u njegovih 1.235 kruzoka"
        );
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_test_fallback_to_other_option_in_plurals() {
        // Use Arabic plural rules since they have all six cases.
        // Only locale and numbers matter, the actual language of the message
        // does not.
        let mut fmt = MessageFormat::new(
            "{NUM_MINUTES, plural, other {# minutes}}", /*, locale, "ar"*/
        );

        // These numbers exercise all cases for the arabic plural rules.
        assert_eq!(fmt.format_with_params([("NUM_MINUTES", "0")]), "0 minutes");
        assert_eq!(fmt.format_with_params([("NUM_MINUTES", "1")]), "1 minutes");
        assert_eq!(fmt.format_with_params([("NUM_MINUTES", "2")]), "2 minutes");
        assert_eq!(fmt.format_with_params([("NUM_MINUTES", "3")]), "3 minutes");
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", "11")]),
            "11 minutes"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", "1.5")]),
            "1.5 minutes"
        );
    }

    #[test]
    fn test_test_pound_shows_number_minus_offset_in_all_cases() {
        let mut fmt = MessageFormat::new(
            "{SOME_NUM, plural, offset:1 =0 {#} =1 {#} =2 {#} one {#} other {#}}",
        );

        assert_eq!(fmt.format_with_params([("SOME_NUM", "0")]), "-1");
        assert_eq!(fmt.format_with_params([("SOME_NUM", "1")]), "0");
        assert_eq!(fmt.format_with_params([("SOME_NUM", "2")]), "1");
        assert_eq!(fmt.format_with_params([("SOME_NUM", "21")]), "20");
    }

    #[test]
    fn test_test_special_characters_in_paramater_dont_change_format() {
        let mut fmt = MessageFormat::new("{SOME_NUM, plural, other {# {GROUP}}}");

        // Test pound sign.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "10"), ("GROUP", "group#1")]),
            "10 group#1"
        );
        // Test other special characters in parameters, like { and }.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "10"), ("GROUP", "} {")]),
            "10 } {"
        );
    }

    #[test]
    fn test_test_missing_or_invalid_plural_parameter() {
        let mut fmt = MessageFormat::new("{SOME_NUM, plural, other {result}}");

        // Key name doesn"t match A != SOME_NUM.
        assert_eq!(
            fmt.format_with_params([("A", "10")]),
            "Undefined parameter - SOME_NUM"
        );

        // Value is not a number.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "Value")]),
            "Invalid parameter - SOME_NUM"
        );
    }

    #[test]
    fn test_test_missing_select_parameter() {
        let mut fmt = MessageFormat::new("{GENDER, select, other {result}}");

        // Key name doesn"t match A != GENDER.
        assert_eq!(
            fmt.format_with_params([("A", "female")]),
            "Undefined parameter - GENDER"
        );
    }

    #[test]
    fn test_test_missing_simple_placeholder() {
        let mut fmt = MessageFormat::new("{result}");

        // Key name doesn"t match A != result.
        assert_eq!(
            fmt.format_with_params([("A", "none")]),
            "Undefined parameter - result"
        );
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_test_plural() {
        let mut fmt = MessageFormat::new(
            "{SOME_NUM, plural,\
            =0 {none}\
            =1 {exactly one}\
            one {# one}\
            few {# few}\
            many {# many}\
            other {# other}\
          }",
            // locale,
            // "ru",
        );

        assert_eq!(fmt.format_with_params([("SOME_NUM", 0)]), "none");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 1)]), "exactly one");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 21)]), "21 one");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 23)]), "23 few");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 17)]), "17 many");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 100)]), "100 many");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 1.4)]), "1,4 other");
        assert_eq!(fmt.format_with_params([("SOME_NUM", "10.0")]), "10 many");
        assert_eq!(fmt.format_with_params([("SOME_NUM", "100.00")]), "100 many");
    }

    #[test]
    fn test_test_plural_with_ignore_pound() {
        let mut fmt = MessageFormat::new("{SOME_NUM, plural, other {# {GROUP}}}");

        // Test pound sign.
        assert_eq!(
            fmt.format_ignoring_pound([("SOME_NUM", "10"), ("GROUP", "group#1")]),
            "# group#1"
        );
        // Test other special characters in parameters, like { and }.
        assert_eq!(
            fmt.format_ignoring_pound([("SOME_NUM", "10"), ("GROUP", "} {")]),
            "# } {"
        );
    }

    #[test]
    fn test_test_simple_plural_with_ignore_pound() {
        let mut fmt = MessageFormat::new(
            "I see {NUM_PEOPLE, plural, offset:1 \
          =0 {no one at all in {PLACE}.} \
          =1 {{PERSON} in {PLACE}.} \
          one {{PERSON} and one other person in {PLACE}.} \
          other {{PERSON} and # other people in {PLACE}.}}",
        );

        assert_eq!(
            fmt.format_ignoring_pound([
                ("NUM_PEOPLE", "100"),
                ("PERSON", "Cibu"),
                ("PLACE", "the cubes")
            ]),
            "I see Cibu and # other people in the cubes."
        );
    }

    #[ignore = "needs locale support"]
    #[test]
    fn test_test_romanian_offset_with_negative_value() {
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, plural, offset,2 \
          one {One #}\
          few {Few #}\
          other {Other #}}",
            // locale,
            // "ro",
        );

        // Checking that the decision is done after the offset is substracted
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", -1)]), "Few -3");
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", 1)]), "One -1");
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", -3)]), "Few -5");
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", 3)]), "One 1");
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", -25)]), "Other -27");
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", 25)]), "Other 23");
    }

    #[ignore = "ordinals are not supported"]
    #[test]
    fn test_test_simple_ordinal() {
        // TOFIX. Ordinal not supported in Dart
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, selectordinal, \
          one {Take the elevator to the #st floor.}\
          two {Take the elevator to the #nd floor.}\
          few {Take the elevator to the #rd floor.}\
          other {Take the elevator to the #th floor.}}",
        );

        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 1)]),
            "Take the elevator to the 1st floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 2)]),
            "Take the elevator to the 2nd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 3)]),
            "Take the elevator to the 3rd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 4)]),
            "Take the elevator to the 4th floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 23)]),
            "Take the elevator to the 23rd floor."
        );
        // Esoteric example.
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 0)]),
            "Take the elevator to the 0th floor."
        );
    }

    #[ignore = "ordinals are not supported"]
    #[test]
    fn test_test_ordinal_with_negative_value() {
        // TOFIX. Ordinal not supported in Dart
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, selectordinal, \
          one {Take the elevator to the #st floor.}\
          two {Take the elevator to the #nd floor.}\
          few {Take the elevator to the #rd floor.}\
          other {Take the elevator to the #th floor.}}",
        );

        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", -1)]),
            "Take the elevator to the -1st floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", -2)]),
            "Take the elevator to the -2nd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", -3)]),
            "Take the elevator to the -3rd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", -4)]),
            "Take the elevator to the -4th floor."
        );
    }

    #[test]
    fn test_test_simple_ordinal_with_ignore_pound() {
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, selectordinal, \
          one {Take the elevator to the #st floor.}\
          two {Take the elevator to the #nd floor.}\
          few {Take the elevator to the #rd floor.}\
          other {Take the elevator to the #th floor.}}",
        );

        assert_eq!(
            fmt.format_ignoring_pound([("NUM_FLOOR", 100)]),
            "Take the elevator to the #th floor."
        );
    }

    #[ignore = "ordinals are not supported"]
    #[test]
    fn test_test_missing_or_invalid_ordinal_parameter() {
        let mut fmt = MessageFormat::new("{SOME_NUM, selectordinal, other {result}}");

        // Key name doesn"t match A != SOME_NUM.
        assert_eq!(
            fmt.format_with_params([("A", "10")]),
            "Undefined or invalid parameter - SOME_NUM"
        );

        // Value is not a number.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "Value")]),
            "Undefined or invalid parameter - SOME_NUM"
        );
    }
}
