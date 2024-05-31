use std::collections::HashMap;

use format::Formatter;
use icu::locid::Locale;
use once_cell::sync::Lazy;
use param::{ARGUMENT_NAME, ARGUMENT_OFFSET, OTHER};
use regex::{Captures, Regex};

pub use param::ParamValue;

mod format;
mod param;

static PLURAL_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(\w+)\s*,\s*plural\s*,(?:\s*offset:(\d+))?").unwrap());
static ORDINAL_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(\w+)\s*,\s*selectordinal\s*,").unwrap());
static SELECT_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(\w+)\s*,\s*select\s*,").unwrap());

static KV_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*=?(\w+)\s*").unwrap());
static WHITESPACES_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

#[derive(Debug)]
pub struct MessageFormat<'l> {
    pattern: Option<String>,
    initial_literals: Vec<String>,
    parsed_pattern: Vec<Block>,
    locale: &'l Locale,
}

impl<'l> MessageFormat<'l> {
    pub fn new(pattern: impl Into<String>, locale: &'l Locale) -> Self {
        Self {
            pattern: Some(pattern.into()),
            initial_literals: Default::default(),
            parsed_pattern: Default::default(),
            locale,
        }
    }

    pub fn format(&mut self) -> String {
        self.format_impl(false, None)
    }

    pub fn format_with_params(
        &mut self,
        named_parameters: impl IntoIterator<Item = (impl Into<String>, ParamValue)>,
    ) -> String {
        self.format_impl(
            false,
            Some(
                named_parameters
                    .into_iter()
                    .map(|(k, v)| (k.into(), v))
                    .collect(),
            ),
        )
    }

    pub fn format_ignoring_pound(
        &mut self,
        named_parameters: impl IntoIterator<Item = (impl Into<String>, ParamValue)>,
    ) -> String {
        self.format_impl(
            true,
            Some(
                named_parameters
                    .into_iter()
                    .map(|(k, v)| (k.into(), v))
                    .collect(),
            ),
        )
    }

    fn format_impl(
        &mut self,
        ignore_pound: bool,
        named_parameters: Option<HashMap<String, ParamValue>>,
    ) -> String {
        self.init();

        Formatter::new(
            self.locale,
            &self.initial_literals,
            &self.parsed_pattern,
            ignore_pound,
        )
        .format(named_parameters)
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

    fn parse_select_block(&mut self, pattern: &str) -> HashMap<ParamValue, Vec<Block>> {
        let mut argument_name = None;
        let pattern = SELECT_BLOCK_RE.replace(pattern, |caps: &Captures| {
            // string, name
            argument_name = Some(caps[1].to_owned());
            ""
        });

        let mut result = HashMap::new();
        result.insert(
            ARGUMENT_NAME,
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
            let key = ParamValue::parse_number(&key).unwrap_or_else(|| key.into_owned().into());
            result.insert(key, value);

            pos += 1;
        }

        assert!(
            result.contains_key(&OTHER),
            "missing other key in select statement"
        );

        result
    }

    fn parse_plural_block(&mut self, pattern: &str) -> HashMap<ParamValue, Vec<Block>> {
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
        result.insert(ARGUMENT_NAME, vec![Block::String(argument_name.unwrap())]);
        result.insert(
            ARGUMENT_OFFSET,
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
            let key = ParamValue::parse_number(&key).unwrap_or_else(|| key.into_owned().into());
            result.insert(key, value);

            pos += 1;
        }

        assert!(
            result.contains_key(&OTHER),
            "missing other key in plural statement"
        );

        result
    }

    fn parse_ordinal_block(&mut self, pattern: &str) -> HashMap<ParamValue, Vec<Block>> {
        let mut argument_name = None;
        let pattern = ORDINAL_BLOCK_RE.replace(pattern, |caps: &Captures| {
            argument_name = Some(caps[1].to_owned());
            ""
        });

        let mut result = HashMap::new();
        result.insert(ARGUMENT_NAME, vec![Block::String(argument_name.unwrap())]);
        result.insert(ARGUMENT_OFFSET, vec![Block::String("0".to_owned())]);

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
            let key = ParamValue::parse_number(&key).unwrap_or_else(|| key.into_owned().into());
            result.insert(key, value);

            pos += 1;
        }

        assert!(
            result.contains_key(&OTHER),
            "missing other key in ordinal statement"
        );

        result
    }
}

fn placeholder(idx: usize) -> String {
    const LITERAL_PLACEHOLDER: &str = "\u{FDDF}_";
    format!("_{LITERAL_PLACEHOLDER}{idx}_")
}

#[derive(Debug)]
enum Block {
    Select(HashMap<ParamValue, Vec<Block>>),
    Plural(HashMap<ParamValue, Vec<Block>>),
    Ordinal(HashMap<ParamValue, Vec<Block>>),
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
    use icu::locid::locale;

    use super::*;

    #[test]
    fn test_empty_pattern() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("", &locale);
        assert_eq!(fmt.format(), "");
    }

    #[test]
    #[should_panic(expected = "No matching { for }")]
    fn test_missing_left_curly_brace() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("\'\'{}}", &locale);
        fmt.format();
    }

    #[test]
    #[should_panic(expected = "There are mismatched { or } in the pattern")]
    fn test_too_many_left_curly_braces() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{} {", &locale);
        fmt.format();
    }

    #[test]
    fn test_simple_replacement() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("New York in {SEASON} is nice.", &locale);
        assert_eq!(
            fmt.format_with_params([("SEASON", "the Summer".into())]),
            "New York in the Summer is nice."
        );
    }

    #[test]
    fn test_simple_select() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{GENDER, select,\
            male {His}\
            female {Her}\
            other {Their}} \
            bicycle is {GENDER, select, male {blue} female {red} other {green}}.",
            &locale,
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "male".into())]),
            "His bicycle is blue."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female".into())]),
            "Her bicycle is red."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "other".into())]),
            "Their bicycle is green."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "any".into())]),
            "Their bicycle is green."
        );
    }

    #[test]
    fn test_simple_plural() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "I see {NUM_PEOPLE, plural, offset:1 \
            =0 {no one at all in {PLACE}.} \
            =1 {{PERSON} in {PLACE}.} \
            one {{PERSON} and one other person in {PLACE}.} \
            other {{PERSON} and # other people in {PLACE}.}}",
            &locale,
        );
        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", 0.into()), ("PLACE", "Belgrade".into())]),
            "I see no one at all in Belgrade."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 1.into()),
                ("PERSON", "Markus".into()),
                ("PLACE", "Berlin".into())
            ]),
            "I see Markus in Berlin."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 2.into()),
                ("PERSON", "Mark".into()),
                ("PLACE", "Athens".into())
            ]),
            "I see Mark and one other person in Athens."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 100.into()),
                ("PERSON", "Cibu".into()),
                ("PLACE", "the cubes".into())
            ]),
            "I see Cibu and 99 other people in the cubes."
        );
    }

    #[test]
    fn test_select_nested_in_plural() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{CIRCLES, plural, \
        one {{GENDER, select, \
          female {{WHO} added you to her circle} \
          other  {{WHO} added you to his circle}}} \
        other {{GENDER, select,
          female {{WHO} added you to her # circles} \
          other  {{WHO} added you to his # circles}}}}",
            &locale,
        );
        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "female".into()),
                ("WHO", "Jelena".into()),
                ("CIRCLES", 1.into())
            ]),
            "Jelena added you to her circle",
        );
        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "male".into()),
                ("WHO", "Milan".into()),
                ("CIRCLES", 1234.into())
            ]),
            "Milan added you to his 1,234 circles",
        );
    }

    #[test]
    fn test_plural_nested_in_select() {
        // Added offset just for testing purposes. It doesn't make sense to have it otherwise.
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{GENDER, select, \
        female {{NUM_GROUPS, plural, \
          one {{WHO} added you to her group} \
          other {{WHO} added you to her # groups}}} \
        other {{NUM_GROUPS, plural, offset,1\
          one {{WHO} added you to his group} \
          other {{WHO} added you to his # groups}}}}",
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "female".into()),
                ("WHO", "Jelena".into()),
                ("NUM_GROUPS", 1.into())
            ]),
            "Jelena added you to her group",
        );
        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "male".into()),
                ("WHO", "Milan".into()),
                ("NUM_GROUPS", 1234.into())
            ]),
            "Milan added you to his 1,234 groups",
        );
    }

    #[test]
    fn test_literal_open_curly_brace() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "Anna's house has '{0} and # in the roof' and {NUM_COWS} cows.",
            &locale,
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", 5.into())]),
            "Anna's house has {0} and # in the roof and 5 cows."
        );
    }

    #[test]
    fn test_literal_closed_curly_brace() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "Anna's house has '{'0'} and # in the roof' and {NUM_COWS} cows.",
            &locale,
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", 5.into())]),
            "Anna's house has {0} and # in the roof and 5 cows."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", 8.into())]),
            "Anna's house has {0} and # in the roof and 8 cows."
        );
    }

    #[test]
    fn test_literal_pound_sign() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "Anna's house has '{0}' and '# in the roof' and {NUM_COWS} cows.",
            &locale,
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", 5.into())]),
            "Anna's house has {0} and # in the roof and 5 cows."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", 10.into())]),
            "Anna's house has {0} and # in the roof and 10 cows."
        );
    }

    #[test]
    fn test_no_literals_for_single_quotes() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("Anna's house 'has {NUM_COWS} cows'.", &locale);
        assert_eq!(
            fmt.format_with_params([("NUM_COWS", 5.into())]),
            "Anna's house 'has 5 cows'."
        );
    }

    #[test]
    fn test_consecutive_single_quotes_are_replaced_with_one_single_quote() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("Anna''s house a'{''''b'", &locale);
        assert_eq!(fmt.format(), "Anna's house a{''b");
    }

    #[test]
    fn test_test_consecutive_single_quotes_before_special_char_dont_create_literal() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("a''{NUM_COWS}'b", &locale);
        assert_eq!(fmt.format_with_params([("NUM_COWS", 5.into())]), "a'5'b");
    }

    #[test]
    fn test_serbian_simple_select() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{GENDER, select, female {Njen} other {Njegov}} bicikl je \
             {GENDER, select, female {crven} other {plav}}.",
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([("GENDER", "male".into())]),
            "Njegov bicikl je plav."
        );
        assert_eq!(
            fmt.format_with_params([("GENDER", "female".into())]),
            "Njen bicikl je crven."
        );
    }

    #[test]
    fn test_serbian_simple_plural() {
        let locale = locale!("sr");
        let mut fmt = MessageFormat::new(
            "Ja {NUM_PEOPLE, plural, offset:1 \
            =0 {ne vidim nikoga} \
            =1 {vidim {PERSON}} \
            one {vidim {PERSON} i jos # osobu} \
            few {vidim {PERSON} i jos # osobe} \
            many {vidim {PERSON} i jos # osoba} \
            other {vidim {PERSON} i jos # osoba}} \
          u {PLACE}.",
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", 0.into()), ("PLACE", "Beogradu".into())]),
            "Ja ne vidim nikoga u Beogradu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 1.into()),
                ("PERSON", "Markusa".into()),
                ("PLACE", "Berlinu".into())
            ]),
            "Ja vidim Markusa u Berlinu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 2.into()),
                ("PERSON", "Marka".into()),
                ("PLACE", "Atini".into())
            ]),
            "Ja vidim Marka i jos 1 osobu u Atini."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 4.into()),
                ("PERSON", "Petra".into()),
                ("PLACE", "muzeju".into())
            ]),
            "Ja vidim Petra i jos 3 osobe u muzeju."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 100.into()),
                ("PERSON", "Cibua".into()),
                ("PLACE", "bazenu".into())
            ]),
            "Ja vidim Cibua i jos 99 osoba u bazenu."
        );
    }

    #[test]
    fn test_test_serbian_simple_plural_no_offset() {
        let locale = locale!("sr");
        let mut fmt = MessageFormat::new(
            "Ja {NUM_PEOPLE, plural, \
            =0 {ne vidim nikoga} \
            =1 {vidim {PERSON}} \
            one {vidim {PERSON} i jos # osobu} \
            few {vidim {PERSON} i jos # osobe} \
            many {vidim {PERSON} i jos # osoba} \
            other {vidim {PERSON} i jos # osoba}} \
          u {PLACE}.",
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([("NUM_PEOPLE", 0.into()), ("PLACE", "Beogradu".into())]),
            "Ja ne vidim nikoga u Beogradu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 1.into()),
                ("PERSON", "Markusa".into()),
                ("PLACE", "Berlinu".into())
            ]),
            "Ja vidim Markusa u Berlinu."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 21.into()),
                ("PERSON", "Marka".into()),
                ("PLACE", "Atini".into())
            ]),
            "Ja vidim Marka i jos 21 osobu u Atini."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 3.into()),
                ("PERSON", "Petra".into()),
                ("PLACE", "muzeju".into())
            ]),
            "Ja vidim Petra i jos 3 osobe u muzeju."
        );
        assert_eq!(
            fmt.format_with_params([
                ("NUM_PEOPLE", 100.into()),
                ("PERSON", "Cibua".into()),
                ("PLACE", "bazenu".into())
            ]),
            "Ja vidim Cibua i jos 100 osoba u bazenu."
        );
    }

    #[test]
    fn test_test_serbian_select_nested_in_plural() {
        let locale = locale!("sr");
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
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "female".into()),
                ("WHO", "Jelena".into()),
                ("CIRCLES", 21.into())
            ]),
            "Jelena vas je dodala u njen 21 kruzok"
        );
        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "female".into()),
                ("WHO", "Jelena".into()),
                ("CIRCLES", 3.into())
            ]),
            "Jelena vas je dodala u njena 3 kruzoka"
        );
        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "female".into()),
                ("WHO", "Jelena".into()),
                ("CIRCLES", 5.into())
            ]),
            "Jelena vas je dodala u njenih 5 kruzoka"
        );
        assert_eq!(
            fmt.format_with_params([
                ("GENDER", "male".into()),
                ("WHO", "Milan".into()),
                ("CIRCLES", 1235.into())
            ]),
            "Milan vas je dodao u njegovih 1.235 kruzoka"
        );
    }

    #[test]
    fn test_test_fallback_to_other_option_in_plurals() {
        // Use Arabic plural rules since they have all six cases.
        // Only locale and numbers matter, the actual language of the message
        // does not.
        let locale = locale!("ar-DZ");
        let mut fmt = MessageFormat::new("{NUM_MINUTES, plural, other {# minutes}}", &locale);

        // These numbers exercise all cases for the arabic plural rules.
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", 0.into())]),
            "0 minutes"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", 1.into())]),
            "1 minutes"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", 2.into())]),
            "2 minutes"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", 3.into())]),
            "3 minutes"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", 11.into())]),
            "11 minutes"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_MINUTES", 1.5.into())]),
            "1,5 minutes"
        );
    }

    #[test]
    fn test_test_pound_shows_number_minus_offset_in_all_cases() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{SOME_NUM, plural, offset:1 =0 {#} =1 {#} =2 {#} one {#} other {#}}",
            &locale,
        );

        assert_eq!(fmt.format_with_params([("SOME_NUM", 0.into())]), "-1");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 1.into())]), "0");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 2.into())]), "1");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 21.into())]), "20");
    }

    #[test]
    fn test_test_special_characters_in_paramater_dont_change_format() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{SOME_NUM, plural, other {# {GROUP}}}", &locale);

        // Test pound sign.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", 10.into()), ("GROUP", "group#1".into())]),
            "10 group#1"
        );
        // Test other special characters in parameters, like { and }.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", 10.into()), ("GROUP", "} {".into())]),
            "10 } {"
        );
    }

    #[test]
    fn test_test_missing_or_invalid_plural_parameter() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{SOME_NUM, plural, other {result}}", &locale);

        // Key name doesn"t match A != SOME_NUM.
        assert_eq!(
            fmt.format_with_params([("A", 10.into())]),
            "Undefined parameter - SOME_NUM"
        );

        // Value is not a number.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "Value".into())]),
            "Invalid parameter - SOME_NUM"
        );
    }

    #[test]
    fn test_test_missing_select_parameter() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{GENDER, select, other {result}}", &locale);

        // Key name doesn"t match A != GENDER.
        assert_eq!(
            fmt.format_with_params([("A", "female".into())]),
            "Undefined parameter - GENDER"
        );
    }

    #[test]
    fn test_test_missing_simple_placeholder() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{result}", &locale);

        // Key name doesn"t match A != result.
        assert_eq!(
            fmt.format_with_params([("A", "none".into())]),
            "Undefined parameter - result"
        );
    }

    #[test]
    fn test_test_plural() {
        let locale = locale!("ru");
        let mut fmt = MessageFormat::new(
            "{SOME_NUM, plural,\
            =0 {none}\
            =1 {exactly one}\
            one {# one}\
            few {# few}\
            many {# many}\
            other {# other}\
          }",
            &locale,
        );

        assert_eq!(fmt.format_with_params([("SOME_NUM", 0.into())]), "none");
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", 1.into())]),
            "exactly one"
        );
        assert_eq!(fmt.format_with_params([("SOME_NUM", 21.into())]), "21 one");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 23.into())]), "23 few");
        assert_eq!(fmt.format_with_params([("SOME_NUM", 17.into())]), "17 many");
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", 100.into())]),
            "100 many"
        );
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", 1.4.into())]),
            "1,4 other"
        );
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "10.0".into())]),
            "10 many"
        );
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "100.00".into())]),
            "100 many"
        );
    }

    #[test]
    fn test_test_plural_with_ignore_pound() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{SOME_NUM, plural, other {# {GROUP}}}", &locale);

        // Test pound sign.
        assert_eq!(
            fmt.format_ignoring_pound([("SOME_NUM", 10.into()), ("GROUP", "group#1".into())]),
            "# group#1"
        );
        // Test other special characters in parameters, like { and }.
        assert_eq!(
            fmt.format_ignoring_pound([("SOME_NUM", 10.into()), ("GROUP", "} {".into())]),
            "# } {"
        );
    }

    #[test]
    fn test_test_simple_plural_with_ignore_pound() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "I see {NUM_PEOPLE, plural, offset:1 \
          =0 {no one at all in {PLACE}.} \
          =1 {{PERSON} in {PLACE}.} \
          one {{PERSON} and one other person in {PLACE}.} \
          other {{PERSON} and # other people in {PLACE}.}}",
            &locale,
        );

        assert_eq!(
            fmt.format_ignoring_pound([
                ("NUM_PEOPLE", 100.into()),
                ("PERSON", "Cibu".into()),
                ("PLACE", "the cubes".into())
            ]),
            "I see Cibu and # other people in the cubes."
        );
    }

    #[test]
    fn test_test_romanian_offset_with_negative_value() {
        let locale = locale!("ro");
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, plural, offset:2 \
          one {One #}\
          few {Few #}\
          other {Other #}}",
            &locale,
        );

        // Checking that the decision is done after the offset is substracted
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-1_i64).into())]),
            "Few -3"
        );
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", 1.into())]), "One -1");
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-3_i64).into())]),
            "Few -5"
        );
        assert_eq!(fmt.format_with_params([("NUM_FLOOR", 3.into())]), "One 1");
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-25_i64).into())]),
            "Other -27"
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 25.into())]),
            "Other 23"
        );
    }

    #[ignore = "ordinals are not supported"]
    #[test]
    fn test_test_simple_ordinal() {
        // TOFIX. Ordinal not supported in Dart
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, selectordinal, \
          one {Take the elevator to the #st floor.}\
          two {Take the elevator to the #nd floor.}\
          few {Take the elevator to the #rd floor.}\
          other {Take the elevator to the #th floor.}}",
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 1.into())]),
            "Take the elevator to the 1st floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 2.into())]),
            "Take the elevator to the 2nd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 3.into())]),
            "Take the elevator to the 3rd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 4.into())]),
            "Take the elevator to the 4th floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 23.into())]),
            "Take the elevator to the 23rd floor."
        );
        // Esoteric example.
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", 0.into())]),
            "Take the elevator to the 0th floor."
        );
    }

    #[ignore = "ordinals are not supported"]
    #[test]
    fn test_test_ordinal_with_negative_value() {
        // TOFIX. Ordinal not supported in Dart
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, selectordinal, \
          one {Take the elevator to the #st floor.}\
          two {Take the elevator to the #nd floor.}\
          few {Take the elevator to the #rd floor.}\
          other {Take the elevator to the #th floor.}}",
            &locale,
        );

        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-1_i64).into())]),
            "Take the elevator to the -1st floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-2_i64).into())]),
            "Take the elevator to the -2nd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-3_i64).into())]),
            "Take the elevator to the -3rd floor."
        );
        assert_eq!(
            fmt.format_with_params([("NUM_FLOOR", (-4_i64).into())]),
            "Take the elevator to the -4th floor."
        );
    }

    #[test]
    fn test_test_simple_ordinal_with_ignore_pound() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new(
            "{NUM_FLOOR, selectordinal, \
          one {Take the elevator to the #st floor.}\
          two {Take the elevator to the #nd floor.}\
          few {Take the elevator to the #rd floor.}\
          other {Take the elevator to the #th floor.}}",
            &locale,
        );

        assert_eq!(
            fmt.format_ignoring_pound([("NUM_FLOOR", 100.into())]),
            "Take the elevator to the #th floor."
        );
    }

    #[ignore = "ordinals are not supported"]
    #[test]
    fn test_test_missing_or_invalid_ordinal_parameter() {
        let locale = locale!("en");
        let mut fmt = MessageFormat::new("{SOME_NUM, selectordinal, other {result}}", &locale);

        // Key name doesn"t match A != SOME_NUM.
        assert_eq!(
            fmt.format_with_params([("A", 10.into())]),
            "Undefined or invalid parameter - SOME_NUM"
        );

        // Value is not a number.
        assert_eq!(
            fmt.format_with_params([("SOME_NUM", "Value".into())]),
            "Undefined or invalid parameter - SOME_NUM"
        );
    }
}
