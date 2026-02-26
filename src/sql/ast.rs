use crate::parser::{SelectNode, Filter, OrderNode};

#[derive(Debug, Clone)]
pub struct QueryAst {
    pub table: String,             // target table name
    pub schema: String,            // default "public"
    pub operation: Operation,
    pub select: Vec<SelectNode>,   // from ?select= param
    pub filters: Vec<Filter>,      // from column=op.val params
    pub order: Vec<OrderNode>,     // from ?order= param
    pub limit: Option<i64>,        // from ?limit= param
    pub offset: Option<i64>,       // from ?offset= param
    pub count: CountMethod,        // from Prefer: count=exact/planned/estimated
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    Select,
    Insert { returning: bool },
    Update { returning: bool },
    Delete { returning: bool },
    Upsert { on_conflict: String, returning: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CountMethod {
    None,
    Exact,
    Planned,
    Estimated,
}
