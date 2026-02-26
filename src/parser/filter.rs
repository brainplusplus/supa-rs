use nom::{
    branch::alt,
    bytes::complete::{escaped, tag, take_while1},
    character::complete::{char, none_of, multispace0},
    combinator::{map, opt, recognize, value},
    multi::separated_list1,
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Eq, Neq, Lt, Lte, Gt, Gte,
    Like, Ilike, Match, Imatch,
    Is, IsNot,
    In, NotIn,
    Contains, ContainedBy,
    Overlaps,
    Fts, Plfts, Phfts, Wfts,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    Single(String),
    List(Vec<String>),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnFilter {
    pub column: String,
    pub negated: bool,
    pub operator: Operator,
    pub value: FilterValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Column(ColumnFilter),
    And(Vec<Filter>),
    Or(Vec<Filter>),
}

pub fn parse_filter(input: &str) -> Result<Filter, String> {
    match parse_logical_or_column(input) {
        Ok((rem, f)) if rem.is_empty() => Ok(f),
        Ok((rem, _)) => Err(format!("Trailing characters: {}", rem)),
        Err(e) => Err(format!("Parse error: {:?}", e)),
    }
}

fn parse_logical_or_column(input: &str) -> IResult<&str, Filter> {
    alt((parse_logical, parse_column_filter))(input)
}

fn parse_logical(input: &str) -> IResult<&str, Filter> {
    alt((
        map(
            preceded(tag("and=("), pair(separated_list1(char(','), parse_logical_or_column), char(')'))),
            |(filters, _)| Filter::And(filters)
        ),
        map(
            preceded(tag("or=("), pair(separated_list1(char(','), parse_logical_or_column), char(')'))),
            |(filters, _)| Filter::Or(filters)
        )
    ))(input)
}

fn parse_operator(input: &str) -> IResult<&str, Operator> {
    alt((
        value(Operator::Eq, tag("eq")),
        value(Operator::Neq, tag("neq")),
        value(Operator::Lte, tag("lte")), // Must precede lt
        value(Operator::Lt, tag("lt")),
        value(Operator::Gte, tag("gte")), // Must precede gt
        value(Operator::Gt, tag("gt")),
        value(Operator::Ilike, tag("ilike")),
        value(Operator::Like, tag("like")),
        value(Operator::Imatch, tag("imatch")),
        value(Operator::Match, tag("match")),
        value(Operator::IsNot, tag("is.not")), // Must precede is
        value(Operator::Is, tag("is")),
        value(Operator::NotIn, tag("not.in")), // Must precede in
        value(Operator::In, tag("in")),
        value(Operator::Contains, tag("cs")),
        value(Operator::ContainedBy, tag("cd")),
        value(Operator::Overlaps, tag("ov")),
        value(Operator::Fts, tag("fts")),
        value(Operator::Plfts, tag("plfts")),
        value(Operator::Phfts, tag("phfts")),
        value(Operator::Wfts, tag("wfts")),
    ))(input)
}

fn parse_column(input: &str) -> IResult<&str, String> {
    map(take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '>'), |s: &str| s.to_string())(input)
}

fn parse_negation(input: &str) -> IResult<&str, bool> {
    map(opt(tag("not.")), |o| o.is_some())(input)
}

fn parse_quoted_value(input: &str) -> IResult<&str, String> {
    delimited(
        char('"'),
        map(opt(escaped(none_of("\\\""), '\\', char('"'))), |s: Option<&str>| s.unwrap_or("").to_string()),
        char('"')
    )(input)
}

fn parse_unquoted_value(input: &str) -> IResult<&str, String> {
    map(take_while1(|c: char| c != ',' && c != ')' && c != '(' && c != '"'), |s: &str| s.to_string())(input)
}

fn parse_list_item(input: &str) -> IResult<&str, String> {
    alt((parse_quoted_value, parse_unquoted_value))(input)
}

fn parse_value_list(input: &str) -> IResult<&str, Vec<String>> {
    delimited(
        char('('),
        separated_list1(char(','), parse_list_item),
        char(')')
    )(input)
}

fn parse_column_filter(input: &str) -> IResult<&str, Filter> {
    let (input, column) = parse_column(input)?;
    let (input, _) = char('.')(input)?;
    let (input, negated) = parse_negation(input)?;
    let (input, op) = parse_operator(input)?;

    if matches!(op, Operator::In | Operator::NotIn) {
        let (input, _) = char('.')(input)?;
        let (input, list) = parse_value_list(input)?;
        Ok((input, Filter::Column(ColumnFilter { column, negated, operator: op, value: FilterValue::List(list) })))
    } else {
        let (input, has_dot) = opt(char('.'))(input)?;

        // is and is.not don't have a value separated by a dot in some formats, but for "is.null" it's treated as value "null"
        if !has_dot.is_some() && (matches!(op, Operator::Is | Operator::IsNot)) {
            // "is" or "is.not" without a value? Wait, Supabase does `is.null` or `is.true`. The `.null` is matched as value.
            // Oh, if it's `is.null`, the `parse_operator` wouldn't consume `.null`.
            // Wait, my `parse_operator` for `is` matched "is", then `.`, then `"null"`
            // If it's just `is`, then it has dot and "null".
            // Since we already matched `is` or `is.not`, the next should be `.null`, `.true`, etc.
        }

        let (input, val) = if has_dot.is_some() {
            alt((parse_quoted_value, parse_unquoted_value))(input)?
        } else {
            (input, "".to_string())
        };

        let value = if (matches!(op, Operator::Is | Operator::IsNot)) && val.to_lowercase() == "null" {
            FilterValue::Null
        } else {
            FilterValue::Single(val)
        };

        Ok((input, Filter::Column(ColumnFilter { column, negated, operator: op, value })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_column_basic() {
        let parsed = parse_filter("age.lt.18").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.column, "age");
            assert_eq!(c.negated, false);
            assert_eq!(c.operator, Operator::Lt);
            assert_eq!(c.value, FilterValue::Single("18".into()));
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_negated_in() {
        let parsed = parse_filter("col.not.in.(a,b,c)").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.column, "col");
            assert_eq!(c.negated, true);
            assert_eq!(c.operator, Operator::In);
            assert_eq!(c.value, FilterValue::List(vec!["a".into(), "b".into(), "c".into()]));
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_quoted_in() {
        let parsed = parse_filter("name.in.(\"John,Doe\",\"Jane\")").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.operator, Operator::In);
            assert_eq!(c.value, FilterValue::List(vec!["John,Doe".into(), "Jane".into()]));
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_is_null() {
        let parsed = parse_filter("col.is.null").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.column, "col");
            assert_eq!(c.operator, Operator::Is);
            assert_eq!(c.value, FilterValue::Null);
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_is_not_null() {
        let parsed = parse_filter("col.is.not.null").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.column, "col");
            assert_eq!(c.operator, Operator::IsNot);
            assert_eq!(c.value, FilterValue::Null);
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_like() {
        let parsed = parse_filter("col.like.*john*").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.column, "col");
            assert_eq!(c.operator, Operator::Like);
            assert_eq!(c.value, FilterValue::Single("*john*".into()));
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_json_path() {
        let parsed = parse_filter("data->>key.eq.val").unwrap();
        if let Filter::Column(c) = parsed {
            assert_eq!(c.column, "data->>key");
            assert_eq!(c.operator, Operator::Eq);
            assert_eq!(c.value, FilterValue::Single("val".into()));
        } else {
            panic!("Expected column filter");
        }
    }

    #[test]
    fn test_filter_or() {
        let parsed = parse_filter("or=(age.lt.18,name.eq.bob)").unwrap();
        if let Filter::Or(filters) = parsed {
            assert_eq!(filters.len(), 2);
            if let Filter::Column(c1) = &filters[0] {
                assert_eq!(c1.column, "age");
            } else { panic!("Expected col"); }
            if let Filter::Column(c2) = &filters[1] {
                assert_eq!(c2.column, "name");
            } else { panic!("Expected col"); }
        } else {
            panic!("Expected OR filter");
        }
    }
}
