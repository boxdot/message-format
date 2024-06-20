use std::collections::HashMap;

use icu::{
    locid::Locale,
    plurals::{PluralCategory, PluralOperands, PluralRules},
};
use icu_decimal::FixedDecimalFormatter;

use crate::{placeholder, Block, ParamValue, OTHER};

#[derive(Debug)]
pub(crate) struct Formatter<'a> {
    locale: &'a Locale,
    initial_literals: &'a Vec<String>,
    parsed_pattern: &'a Vec<Block>,
    ignore_pound: bool,
    fdf: Option<FixedDecimalFormatter>,
}

impl<'a> Formatter<'a> {
    pub(crate) fn new(
        locale: &'a Locale,
        initial_literals: &'a Vec<String>,
        parsed_pattern: &'a Vec<Block>,
        ignore_pound: bool,
    ) -> Self {
        Self {
            locale,
            parsed_pattern,
            initial_literals,
            ignore_pound,
            fdf: Default::default(),
        }
    }

    fn fixed_decimal_formatter(&mut self) -> &FixedDecimalFormatter {
        self.fdf.get_or_insert_with(|| {
            FixedDecimalFormatter::try_new(&self.locale.into(), Default::default())
                .expect("missing locale")
        })
    }

    pub(crate) fn format(
        &mut self,
        named_parameters: Option<HashMap<String, ParamValue>>,
    ) -> String {
        if self.parsed_pattern.is_empty() {
            return String::new();
        }

        let mut literals = self.initial_literals.clone();

        let mut message_parts = Vec::new();
        self.format_block(
            self.parsed_pattern,
            named_parameters.as_ref().unwrap_or(&HashMap::new()),
            &mut literals,
            &mut message_parts,
        );
        let mut message = message_parts.join("");

        if !self.ignore_pound {
            assert!(!message.contains('#'), "not all # were replaced");
        }

        while let Some(literal) = literals.pop() {
            let placeholder = placeholder(literals.len());
            message = message.replacen(&placeholder, &literal, 1);
        }

        message
    }

    fn format_block(
        &mut self,
        parsed_blocks: &[Block],
        named_parameters: &HashMap<String, ParamValue>,
        literals: &mut Vec<String>,
        result: &mut Vec<String>,
    ) {
        for current_pattern in parsed_blocks {
            match current_pattern {
                Block::String(value) => {
                    result.push(value.clone());
                }
                Block::Simple(value) => {
                    self.format_simple_placeholder(value, named_parameters, literals, result);
                }
                Block::Select(map_pattern) => {
                    self.format_select_block(map_pattern, named_parameters, literals, result);
                }
                Block::Plural(value) => {
                    self.format_plural_ordinal_block(
                        value,
                        named_parameters,
                        literals,
                        plural_rules_select,
                        result,
                    );
                }
                Block::Ordinal(value) => {
                    self.format_plural_ordinal_block(
                        value,
                        named_parameters,
                        literals,
                        ordinal_rules_select,
                        result,
                    );
                }
            }
        }
    }

    fn format_simple_placeholder(
        &self,
        param: &str,
        named_parameters: &HashMap<String, ParamValue>,
        literals: &mut Vec<String>,
        result: &mut Vec<String>,
    ) {
        let Some(value) = named_parameters.get(param) else {
            result.push(format!("Undefined parameter - {param}"));
            return;
        };
        let value = value.format_with_locale(self.locale);
        let placeholder = placeholder(literals.len());
        literals.push(value);
        result.push(placeholder);
    }

    fn format_select_block(
        &mut self,
        parsed_blocks: &HashMap<ParamValue, Vec<Block>>,
        named_parameters: &HashMap<String, ParamValue>,
        literals: &mut Vec<String>,
        result: &mut Vec<String>,
    ) {
        let Some(Block::String(argument_name)) = parsed_blocks
            .get(&"argumentName".to_owned().into())
            .and_then(|b| b.first())
        else {
            panic!("invalid argument name");
        };

        let Some(param) = named_parameters.get(argument_name) else {
            result.push(format!("Undefined parameter - {argument_name}"));
            return;
        };

        let Some(option) = parsed_blocks
            .get(param)
            .or_else(|| parsed_blocks.get(&OTHER))
        else {
            panic!("Invalid option or missing other option for select block");
        };

        self.format_block(option, named_parameters, literals, result);
    }

    fn format_plural_ordinal_block(
        &mut self,
        parsed_blocks: &HashMap<ParamValue, Vec<Block>>,
        named_parameters: &HashMap<String, ParamValue>,
        literals: &mut Vec<String>,
        plural_selector: impl Fn(PluralOperands, &Locale) -> &'static str,
        result: &mut Vec<String>,
    ) {
        let Some(Block::String(argument_name)) = parsed_blocks
            .get(&"argumentName".into())
            .and_then(|b| b.first())
        else {
            panic!("invalid argument name");
        };
        let Some(Block::String(argument_offset)) = parsed_blocks
            .get(&"argumentOffset".into())
            .and_then(|b| b.first())
        else {
            panic!("invalid argument offset");
        };

        let Some(plural_value) = named_parameters.get(argument_name) else {
            result.push(format!("Undefined parameter - {argument_name}"));
            return;
        };

        let Some(plural_value) = plural_value.as_decimal() else {
            result.push(format!("Invalid parameter - {argument_name}"));
            return;
        };

        let Ok(argument_offset) = argument_offset.parse::<f64>() else {
            result.push(format!("Invalid offset - {argument_offset}"));
            return;
        };

        let diff = plural_value - argument_offset;

        let option = match parsed_blocks.get(&named_parameters[argument_name]) {
            Some(option) => option,
            None => {
                let Ok(diff_fixed_decimal) = diff.abs().to_string().parse() else {
                    result.push(format!("Invalid parameter - {diff}"));
                    return;
                };
                let item = plural_selector(diff_fixed_decimal, self.locale);
                let Some(option) = parsed_blocks
                    .get(&item.to_owned().into())
                    .or_else(|| parsed_blocks.get(&OTHER))
                else {
                    panic!("Invalid option or missing other option for plural block");
                };
                option
            }
        };

        let mut plural_result = Vec::new();
        self.format_block(option, named_parameters, literals, &mut plural_result);
        let plural = plural_result.join("");
        if self.ignore_pound {
            result.push(plural);
        } else {
            let diff_str = diff.to_string();
            let diff_formatted = if let Ok(diff_fixed) = diff_str.parse() {
                self.fixed_decimal_formatter().format_to_string(&diff_fixed)
            } else {
                diff_str
            };
            result.push(plural.replace('#', &diff_formatted));
        }
    }
}

fn plural_rules_select(n: PluralOperands, locale: &Locale) -> &'static str {
    let rule = PluralRules::try_new(&locale.into(), icu::plurals::PluralRuleType::Cardinal)
        .expect("missing locale");
    match rule.category_for(n) {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

fn ordinal_rules_select(n: PluralOperands, locale: &Locale) -> &'static str {
    // Ordinals are not supported
    // <https://github.com/dart-lang/i18n/blob/98e7b4aea2e6ff613ec273ca29f58938d9c5b23d/pkgs/intl/lib/message_format.dart#L771>
    plural_rules_select(n, locale)
}
