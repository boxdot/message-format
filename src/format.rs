use std::collections::HashMap;

use icu::{
    locale::Locale,
    plurals::{PluralCategory, PluralOperands, PluralRuleType, PluralRules, PluralRulesOptions},
};
use icu_decimal::DecimalFormatter;

use crate::{Block, OTHER, ParamValue, placeholder};

#[derive(Debug)]
pub(crate) struct Formatter<'a> {
    locale: &'a Locale,
    initial_literals: &'a Vec<String>,
    parsed_pattern: &'a Vec<Block>,
    ignore_pound: bool,
    fdf: Option<DecimalFormatter>,
    plural_rules: Option<PluralRules>,
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
            plural_rules: Default::default(),
        }
    }

    fn fixed_decimal_formatter(&mut self) -> &DecimalFormatter {
        self.fdf.get_or_insert_with(|| {
            DecimalFormatter::try_new(self.locale.into(), Default::default())
                .expect("missing locale")
        })
    }

    fn get_plural_rules(&mut self) -> &PluralRules {
        self.plural_rules.get_or_insert_with(|| {
            PluralRules::try_new(
                self.locale.into(),
                PluralRulesOptions::default().with_type(PluralRuleType::Cardinal),
            )
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
                    self.format_plural_ordinal_block(value, named_parameters, literals, result);
                }
                Block::Ordinal(value) => {
                    // Ordinals are not supported, falls back to cardinal rules.
                    // <https://github.com/dart-lang/i18n/blob/98e7b4aea2e6ff613ec273ca29f58938d9c5b23d/pkgs/intl/lib/message_format.dart#L771>
                    self.format_plural_ordinal_block(value, named_parameters, literals, result);
                }
            }
        }
    }

    fn format_simple_placeholder(
        &mut self,
        param: &str,
        named_parameters: &HashMap<String, ParamValue>,
        literals: &mut Vec<String>,
        result: &mut Vec<String>,
    ) {
        let Some(value) = named_parameters.get(param) else {
            result.push(format!("Undefined parameter - {param}"));
            return;
        };
        let value = value.format_with_fdf(self.fixed_decimal_formatter());
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
            unreachable!("argumentName is always inserted by the parser");
        };

        let Some(param) = named_parameters.get(argument_name) else {
            result.push(format!("Undefined parameter - {argument_name}"));
            return;
        };

        let Some(option) = parsed_blocks
            .get(param)
            .or_else(|| parsed_blocks.get(&OTHER))
        else {
            unreachable!("select block always has an 'other' clause, validated by the parser");
        };

        self.format_block(option, named_parameters, literals, result);
    }

    fn format_plural_ordinal_block(
        &mut self,
        parsed_blocks: &HashMap<ParamValue, Vec<Block>>,
        named_parameters: &HashMap<String, ParamValue>,
        literals: &mut Vec<String>,
        result: &mut Vec<String>,
    ) {
        let Some(Block::String(argument_name)) = parsed_blocks
            .get(&"argumentName".into())
            .and_then(|b| b.first())
        else {
            unreachable!("argumentName is always inserted by the parser");
        };
        let Some(Block::String(argument_offset)) = parsed_blocks
            .get(&"argumentOffset".into())
            .and_then(|b| b.first())
        else {
            unreachable!("argumentOffset is always inserted by the parser");
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
                let Ok(diff_fixed_decimal) = diff.abs().to_string().parse::<PluralOperands>()
                else {
                    result.push(format!("Invalid parameter - {diff}"));
                    return;
                };
                let item = match self.get_plural_rules().category_for(diff_fixed_decimal) {
                    PluralCategory::Zero => "zero",
                    PluralCategory::One => "one",
                    PluralCategory::Two => "two",
                    PluralCategory::Few => "few",
                    PluralCategory::Many => "many",
                    PluralCategory::Other => "other",
                };
                let Some(option) = parsed_blocks
                    .get(&item.to_owned().into())
                    .or_else(|| parsed_blocks.get(&OTHER))
                else {
                    unreachable!(
                        "plural block always has an 'other' clause, validated by the parser"
                    );
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
