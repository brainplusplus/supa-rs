use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::{char, multispace0},
    combinator::{map, opt, recognize},
    multi::separated_list0,
    sequence::{delimited, preceded, tuple},
    IResult,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SelectNode {
    pub name: String,
    pub alias: Option<String>,
    pub cast: Option<String>,
    pub children: Vec<SelectNode>,
    pub json_path: Option<String>,
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '*' || c == '.'
}

fn parse_ident(input: &str) -> IResult<&str, String> {
    map(take_while1(is_ident_char), |s: &str| s.to_string())(input)
}

fn parse_json_path(input: &str) -> IResult<&str, String> {
    map(
        recognize(tuple((
            alt((tag("->>"), tag("->"))),
            take_while1(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '>'),
        ))),
        |s: &str| s.to_string(),
    )(input)
}

fn parse_alias_prefix(input: &str) -> IResult<&str, Option<String>> {
    // Manually peek to avoid generic inference errors with nom::combinator::not
    let (rem, possible_alias) = match take_while1::<_, &str, nom::error::Error<&str>>(|c: char| c.is_ascii_alphanumeric() || c == '_')(input) {
        Ok(res) => res,
        Err(_) => return Ok((input, None)),
    };

    if rem.starts_with(':') && !rem.starts_with("::") {
        // Safe to consume, it's an alias (`:`, not `::`)
        let rem_after_colon = &rem[1..];
        Ok((rem_after_colon, Some(possible_alias.to_string())))
    } else {
        Ok((input, None))
    }
}

fn parse_select_item(mut input: &str) -> IResult<&str, SelectNode> {
    let (rem, _) = multispace0(input)?;
    input = rem;

    // 1. Try alias prefix FIRST (custom manual lookahead for '::')
    let (input, alias) = parse_alias_prefix(input)?;

    // 2. Parse column name
    let (input, name) = parse_ident(input)?;

    // 3. JSON path (->>, ->)
    let (input, json_path) = opt(parse_json_path)(input)?;

    // 4. Cast (::type)
    let (input, cast) = opt(preceded(tag("::"), parse_ident))(input)?;

    // 5. Nested relation
    let (input, opt_children) = opt(delimited(char('('), parse_select_list, char(')')))(input)?;
    let children = opt_children.unwrap_or_default();

    Ok((
        input,
        SelectNode {
            name,
            alias,
            cast,
            children,
            json_path,
        },
    ))
}

fn parse_select_list(input: &str) -> IResult<&str, Vec<SelectNode>> {
    separated_list0(char(','), parse_select_item)(input)
}

pub fn parse_select(input: &str) -> Result<Vec<SelectNode>, String> {
    match parse_select_list(input) {
        Ok((rem, nodes)) => {
            if rem.trim().is_empty() {
                Ok(nodes)
            } else {
                Err(format!("Unparsed remaining input: {}", rem))
            }
        }
        Err(e) => Err(format!("Parse error: {:?}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        // 1. Basic array
        assert_eq!(
            parse_select("id,name").unwrap(),
            vec![
                SelectNode { name: "id".to_string(), alias: None, cast: None, children: vec![], json_path: None },
                SelectNode { name: "name".to_string(), alias: None, cast: None, children: vec![], json_path: None }
            ]
        );

        // 2. Alias
        assert_eq!(
            parse_select("my_id:id").unwrap(),
            vec![
                SelectNode { name: "id".to_string(), alias: Some("my_id".to_string()), cast: None, children: vec![], json_path: None }
            ]
        );

        // 3. Cast
        assert_eq!(
            parse_select("age::text").unwrap(),
            vec![
                SelectNode { name: "age".to_string(), alias: None, cast: Some("text".to_string()), children: vec![], json_path: None }
            ]
        );

        // 4. Json path
        assert_eq!(
            parse_select("metadata->>key").unwrap(),
            vec![
                SelectNode { name: "metadata".to_string(), alias: None, cast: None, children: vec![], json_path: Some("->>key".to_string()) }
            ]
        );

        // 5. Relation children & combinations
        let res = parse_select("id,orders(total::int,items(sku))").unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(res[1].name, "orders");
        assert_eq!(res[1].children.len(), 2);
        assert_eq!(res[1].children[0].name, "total");
        assert_eq!(res[1].children[0].cast, Some("int".to_string()));
        assert_eq!(res[1].children[1].name, "items");
        assert_eq!(res[1].children[1].children[0].name, "sku");
    }
}
