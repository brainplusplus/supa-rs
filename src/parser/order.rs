#[derive(Debug, Clone, PartialEq)]
pub enum Direction { Asc, Desc }

#[derive(Debug, Clone, PartialEq)]
pub enum NullsOrder { First, Last }

#[derive(Debug, Clone, PartialEq)]
pub struct OrderNode {
    pub column: String,
    pub direction: Direction,
    pub nulls: Option<NullsOrder>,
}

pub fn parse_order(input: &str) -> Result<Vec<OrderNode>, String> {
    let mut nodes = Vec::new();
    if input.trim().is_empty() {
        return Ok(nodes);
    }
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() { continue; }
        
        let mut segments = part.split('.');
        let col = segments.next().unwrap().to_string();
        let mut dir = Direction::Asc;
        let mut nulls = None;
        
        for seg in segments {
            match seg.to_lowercase().as_str() {
                "asc" => dir = Direction::Asc,
                "desc" => dir = Direction::Desc,
                "nullsfirst" => nulls = Some(NullsOrder::First),
                "nullslast" => nulls = Some(NullsOrder::Last),
                _ => return Err(format!("Invalid order segment: {}", seg)),
            }
        }
        nodes.push(OrderNode { column: col, direction: dir, nulls });
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_order() {
        assert_eq!(
            parse_order("col.asc.nullsfirst,col2.desc.nullslast").unwrap(),
            vec![
                OrderNode { column: "col".to_string(), direction: Direction::Asc, nulls: Some(NullsOrder::First) },
                OrderNode { column: "col2".to_string(), direction: Direction::Desc, nulls: Some(NullsOrder::Last) }
            ]
        );
        assert_eq!(
            parse_order("id").unwrap(),
            vec![
                OrderNode { column: "id".to_string(), direction: Direction::Asc, nulls: None }
            ]
        );
    }
}
