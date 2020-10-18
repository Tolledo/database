// Copyright 2020 Alex Dukhno
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use description::{Description, DescriptionError, FullTableId, FullTableName, InsertStatement};
use metadata::{DataDefinition, MetadataView};
use sql_model::sql_errors::NotFoundError;
use sqlparser::ast::Statement;
use std::{convert::TryFrom, sync::Arc};

pub struct Analyzer {
    metadata: Arc<DataDefinition>,
}

impl Analyzer {
    pub fn new(metadata: Arc<DataDefinition>) -> Analyzer {
        Analyzer { metadata }
    }

    pub fn describe(&self, statement: &Statement) -> Result<Description, DescriptionError> {
        match statement {
            Statement::Insert { table_name, .. } => {
                let full_table_name = FullTableName::try_from(table_name).unwrap();
                match self.metadata.table_desc((&full_table_name).into()) {
                    Ok(table_def) => Ok(Description::Insert(InsertStatement {
                        table_id: FullTableId::from(table_def.full_table_id()),
                        sql_types: table_def.column_types(),
                    })),
                    Err(NotFoundError::Object) => Err(DescriptionError::table_does_not_exist(&full_table_name)),
                    Err(NotFoundError::Schema) => {
                        Err(DescriptionError::schema_does_not_exist(full_table_name.schema()))
                    }
                }
            }
            _ => unimplemented!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_def::ColumnDefinition;
    use sql_model::{sql_types::SqlType, DEFAULT_CATALOG};
    use sqlparser::ast::{Expr, Ident, ObjectName, Query, SetExpr, Value, Values};
    use std::sync::Arc;

    const SCHEMA: &str = "schema_name";
    const TABLE: &str = "table_name";

    fn ident<S: ToString>(name: S) -> Ident {
        Ident {
            value: name.to_string(),
            quote_style: None,
        }
    }

    fn insert_stmt_with_values<S: ToString>(schema: S, table: S, values: Vec<&'static str>) -> Statement {
        Statement::Insert {
            table_name: ObjectName(vec![ident(schema), ident(table)]),
            columns: vec![],
            source: Box::new(Query {
                ctes: vec![],
                body: SetExpr::Values(Values(vec![values
                    .into_iter()
                    .map(|s| Expr::Value(Value::Number(s.parse().unwrap())))
                    .collect()])),
                order_by: vec![],
                limit: None,
                offset: None,
                fetch: None,
            }),
        }
    }

    fn insert_statement<S: ToString>(schema: S, table: S) -> Statement {
        insert_stmt_with_values(schema, table, vec![])
    }

    #[test]
    fn insert_into_table_under_non_existing_schema() {
        let metadata = Arc::new(DataDefinition::in_memory());
        metadata.create_catalog(DEFAULT_CATALOG);
        let analyzer = Analyzer::new(metadata);
        let description = analyzer.describe(&insert_statement("non_existent_schema", "non_existent_table"));

        assert_eq!(
            description,
            Err(DescriptionError::schema_does_not_exist(&"non_existent_schema"))
        )
    }

    #[test]
    fn insert_into_non_existing_table() {
        let metadata = Arc::new(DataDefinition::in_memory());
        metadata.create_catalog(DEFAULT_CATALOG);
        metadata.create_schema(DEFAULT_CATALOG, SCHEMA);
        let analyzer = Analyzer::new(metadata);
        let description = analyzer.describe(&insert_statement(SCHEMA, "non_existent"));

        assert_eq!(
            description,
            Err(DescriptionError::table_does_not_exist(&format!(
                "{}.{}",
                SCHEMA, "non_existent"
            )))
        );
    }

    #[test]
    fn insert_into_existing_table_without_columns() {
        let metadata = Arc::new(DataDefinition::in_memory());
        metadata.create_catalog(DEFAULT_CATALOG);
        let schema_id = match metadata.create_schema(DEFAULT_CATALOG, SCHEMA) {
            Some((_, Some(schema_id))) => schema_id,
            _ => panic!(),
        };
        let table_id = match metadata.create_table(DEFAULT_CATALOG, SCHEMA, TABLE, &[]) {
            Some((_, Some((_, Some(table_id))))) => table_id,
            _ => panic!(),
        };
        let analyzer = Analyzer::new(metadata);
        let description = analyzer.describe(&insert_statement(SCHEMA, TABLE));

        assert_eq!(
            description,
            Ok(Description::Insert(InsertStatement {
                table_id: FullTableId::from((schema_id, table_id)),
                sql_types: vec![]
            }))
        );
    }

    #[test]
    fn insert_into_existing_table_with_column() {
        let metadata = Arc::new(DataDefinition::in_memory());
        metadata.create_catalog(DEFAULT_CATALOG);
        let schema_id = match metadata.create_schema(DEFAULT_CATALOG, SCHEMA) {
            Some((_, Some(schema_id))) => schema_id,
            _ => panic!(),
        };
        let table_id = match metadata.create_table(
            DEFAULT_CATALOG,
            SCHEMA,
            TABLE,
            &[ColumnDefinition::new("col", SqlType::SmallInt(i16::min_value()))],
        ) {
            Some((_, Some((_, Some(table_id))))) => table_id,
            _ => panic!(),
        };
        let analyzer = Analyzer::new(metadata);
        let description = analyzer.describe(&insert_stmt_with_values(SCHEMA, TABLE, vec!["1"]));

        assert_eq!(
            description,
            Ok(Description::Insert(InsertStatement {
                table_id: FullTableId::from((schema_id, table_id)),
                sql_types: vec![SqlType::SmallInt(i16::min_value())]
            }))
        );
    }
}