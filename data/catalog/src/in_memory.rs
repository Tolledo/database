// Copyright 2020 - present Alex Dukhno
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

use crate::{Cursor, DataCatalog, DataTable, Key, SchemaHandle, Value};
use binary::Binary;
use dashmap::DashMap;
use repr::Datum;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        RwLock,
    },
};

#[derive(Default, Debug)]
pub struct InMemoryTableHandle {
    records: RwLock<BTreeMap<Binary, Binary>>,
    record_ids: AtomicU64,
    column_ords: AtomicU64,
}

impl DataTable for InMemoryTableHandle {
    fn select(&self) -> Cursor {
        self.records
            .read()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Cursor>()
    }

    fn insert(&self, data: Vec<Value>) -> usize {
        let len = data.len();
        let mut rw = self.records.write().unwrap();
        for value in data {
            let record_id = self.record_ids.fetch_add(1, Ordering::SeqCst);
            let key = Binary::pack(&[Datum::from_u64(record_id)]);
            debug_assert!(
                matches!(rw.insert(key, value), None),
                "insert operation should insert nonexistent key"
            );
        }
        len
    }

    fn update(&self, data: Vec<(Key, Value)>) -> usize {
        let len = data.len();
        let mut rw = self.records.write().unwrap();
        for (key, value) in data {
            debug_assert!(
                matches!(rw.insert(key, value), Some(_)),
                "update operation should change already existed key"
            );
        }
        len
    }

    fn delete(&self, data: Vec<Key>) -> usize {
        let mut rw = self.records.write().unwrap();
        let mut size = 0;
        let keys = rw
            .iter()
            .filter(|(key, _value)| data.contains(key))
            .map(|(key, _value)| key.clone())
            .collect::<Vec<Binary>>();
        for key in keys.iter() {
            debug_assert!(matches!(rw.remove(key), Some(_)), "delete operation delete existed key");
            size += 1;
        }
        size
    }

    fn next_column_ord(&self) -> u64 {
        self.column_ords.fetch_add(1, Ordering::SeqCst)
    }
}

#[derive(Default, Debug)]
pub struct InMemorySchemaHandle {
    tables: DashMap<String, InMemoryTableHandle>,
}

impl SchemaHandle for InMemorySchemaHandle {
    type Table = InMemoryTableHandle;

    fn create_table(&self, table_name: &str) -> bool {
        if self.tables.contains_key(table_name) {
            false
        } else {
            self.tables
                .insert(table_name.to_owned(), InMemoryTableHandle::default());
            true
        }
    }

    fn drop_table(&self, table_name: &str) -> bool {
        if !self.tables.contains_key(table_name) {
            false
        } else {
            self.tables.remove(table_name);
            true
        }
    }

    fn work_with<T, F: Fn(&Self::Table) -> T>(&self, table_name: &str, operation: F) -> Option<T> {
        self.tables.get(table_name).map(|table| operation(&*table))
    }
}

#[derive(Default)]
pub struct InMemoryCatalogHandle {
    schemas: DashMap<String, InMemorySchemaHandle>,
}

impl DataCatalog for InMemoryCatalogHandle {
    type Schema = InMemorySchemaHandle;

    fn create_schema(&self, schema_name: &str) -> bool {
        if self.schemas.contains_key(schema_name) {
            false
        } else {
            self.schemas
                .insert(schema_name.to_owned(), InMemorySchemaHandle::default());
            true
        }
    }

    fn drop_schema(&self, schema_name: &str) -> bool {
        if !self.schemas.contains_key(schema_name) {
            false
        } else {
            self.schemas.remove(schema_name);
            true
        }
    }

    fn work_with<T, F: Fn(&Self::Schema) -> T>(&self, schema_name: &str, operation: F) -> Option<T> {
        self.schemas.get(schema_name).map(|schema| operation(&*schema))
    }
}

#[cfg(test)]
mod general_cases {
    use super::*;

    const SCHEMA: &str = "schema_name";
    const SCHEMA_1: &str = "schema_name_1";
    const SCHEMA_2: &str = "schema_name_2";
    const TABLE: &str = "table_name";
    const TABLE_1: &str = "table_name_1";
    const TABLE_2: &str = "table_name_2";
    const DOES_NOT_EXIST: &str = "does_not_exist";

    fn catalog() -> InMemoryCatalogHandle {
        InMemoryCatalogHandle::default()
    }

    #[cfg(test)]
    mod schemas {
        use super::*;

        #[test]
        fn create_schemas_with_different_names() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA_1), true);
            assert_eq!(catalog_handle.work_with(SCHEMA_1, |_schema| 1), Some(1));
            assert_eq!(catalog_handle.create_schema(SCHEMA_2), true);
            assert_eq!(catalog_handle.work_with(SCHEMA_2, |_schema| 2), Some(2));
        }

        #[test]
        fn drop_schema() {
            let catalog_handle = catalog();

            assert!(catalog_handle.create_schema(SCHEMA));
            assert_eq!(catalog_handle.drop_schema(SCHEMA), true);
            assert!(matches!(catalog_handle.work_with(SCHEMA, |_schema| 1), None));
            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert!(matches!(catalog_handle.work_with(SCHEMA, |_schema| 1), Some(1)));
        }

        #[test]
        fn dropping_schema_drops_tables_in_it() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE_1)),
                Some(true)
            );
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE_2)),
                Some(true)
            );

            assert_eq!(catalog_handle.drop_schema(SCHEMA), true);
            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE_1)),
                Some(true)
            );
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE_2)),
                Some(true)
            );
        }

        #[test]
        fn create_schema_with_the_same_name() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(catalog_handle.create_schema(SCHEMA), false);
        }

        #[test]
        fn drop_schema_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.drop_schema(SCHEMA), false);
        }
    }

    #[cfg(test)]
    mod create_table {
        use super::*;

        #[test]
        fn create_tables_with_different_names() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE_1)),
                Some(true)
            );
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE_2)),
                Some(true)
            );
        }

        #[test]
        fn create_tables_with_the_same_name_in_the_same_schema() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(true)
            );
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(false)
            );
        }

        #[test]
        fn create_tables_in_non_existent_schema() {
            let catalog_handle = catalog();

            assert_eq!(
                catalog_handle.work_with(DOES_NOT_EXIST, |schema| schema.create_table(TABLE)),
                None
            );
        }

        #[test]
        fn create_table_with_the_same_name_in_different_namespaces() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA_1), true);
            assert_eq!(catalog_handle.create_schema(SCHEMA_2), true);

            assert_eq!(
                catalog_handle.work_with(SCHEMA_1, |schema| schema.create_table(TABLE)),
                Some(true)
            );
            assert_eq!(
                catalog_handle.work_with(SCHEMA_2, |schema| schema.create_table(TABLE)),
                Some(true)
            );
        }
    }

    #[cfg(test)]
    mod drop_table {
        use super::*;

        #[test]
        fn drop_table() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(true)
            );
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.drop_table(TABLE)),
                Some(true)
            );
        }

        #[test]
        fn drop_table_from_schema_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(
                catalog_handle.work_with(DOES_NOT_EXIST, |schema| schema.drop_table(TABLE)),
                None
            );
        }

        #[test]
        fn drop_table_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |s| s.drop_table(DOES_NOT_EXIST)),
                Some(false)
            );
        }
    }

    #[cfg(test)]
    mod operations_on_table {
        use super::*;

        #[test]
        fn scan_table_that_in_schema_that_does_not_exist() {
            let catalog_handle = catalog();

            assert!(matches!(
                catalog_handle.work_with(DOES_NOT_EXIST, |schema| schema.work_with(TABLE, |table| table.select())),
                None
            ));
        }

        #[test]
        fn scan_table_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert!(matches!(
                catalog_handle.work_with(SCHEMA, |schema| schema
                    .work_with(DOES_NOT_EXIST, |table| table.select())),
                Some(None)
            ));
        }

        #[test]
        fn insert_a_row_into_table_in_schema_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.insert(vec![]))),
                None
            );
        }

        #[test]
        fn insert_a_row_into_table_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.insert(vec![]))),
                Some(None)
            );
        }

        #[test]
        fn insert_row_into_table_and_scan() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(true)
            );

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema
                    .work_with(TABLE, |table| table.insert(vec![Binary::pack(&[Datum::from_u64(1)])]))),
                Some(Some(1))
            );

            assert_eq!(
                catalog_handle
                    .work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.select()))
                    .unwrap()
                    .unwrap()
                    .collect::<Vec<(Key, Value)>>(),
                vec![(Binary::pack(&[Datum::from_u64(0)]), Binary::pack(&[Datum::from_u64(1)]))]
            );
        }

        #[test]
        fn insert_many_rows_into_table_and_scan() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(true)
            );

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.insert(vec![
                    Binary::pack(&[Datum::from_u64(1)]),
                    Binary::pack(&[Datum::from_u64(2)])
                ]))),
                Some(Some(2))
            );

            assert_eq!(
                catalog_handle
                    .work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.select()))
                    .unwrap()
                    .unwrap()
                    .collect::<Vec<(Key, Value)>>(),
                vec![
                    (Binary::pack(&[Datum::from_u64(0)]), Binary::pack(&[Datum::from_u64(1)])),
                    (Binary::pack(&[Datum::from_u64(1)]), Binary::pack(&[Datum::from_u64(2)]))
                ]
            );
        }

        #[test]
        fn delete_from_table_that_in_schema_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(
                catalog_handle.work_with(DOES_NOT_EXIST, |schema| schema
                    .work_with(TABLE, |table| table.delete(vec![]))),
                None
            );
        }

        #[test]
        fn delete_from_table_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema
                    .work_with(DOES_NOT_EXIST, |table| table.delete(vec![]))),
                Some(None)
            );
        }

        #[test]
        fn insert_delete_scan_records_from_table() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(true)
            );

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.insert(vec![
                    Binary::pack(&[Datum::from_u64(1)]),
                    Binary::pack(&[Datum::from_u64(2)])
                ]))),
                Some(Some(2))
            );

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema
                    .work_with(TABLE, |table| table.delete(vec![Binary::pack(&[Datum::from_u64(1)])]))),
                Some(Some(1))
            );

            assert_eq!(
                catalog_handle
                    .work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.select()))
                    .unwrap()
                    .unwrap()
                    .collect::<Vec<(Key, Value)>>(),
                vec![(Binary::pack(&[Datum::from_u64(0)]), Binary::pack(&[Datum::from_u64(1)]))]
            );
        }

        #[test]
        fn update_table_that_in_schema_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(
                catalog_handle.work_with(DOES_NOT_EXIST, |schema| schema
                    .work_with(TABLE, |table| table.update(vec![]))),
                None
            );
        }

        #[test]
        fn update_table_that_does_not_exist() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema
                    .work_with(DOES_NOT_EXIST, |table| table.update(vec![]))),
                Some(None)
            );
        }

        #[test]
        fn insert_update_scan_records_from_table() {
            let catalog_handle = catalog();

            assert_eq!(catalog_handle.create_schema(SCHEMA), true);
            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.create_table(TABLE)),
                Some(true)
            );

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.insert(vec![
                    Binary::pack(&[Datum::from_u64(1)]),
                    Binary::pack(&[Datum::from_u64(2)])
                ]))),
                Some(Some(2))
            );

            assert_eq!(
                catalog_handle.work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.update(vec![(
                    Binary::pack(&[Datum::from_u64(1)]),
                    Binary::pack(&[Datum::from_u64(4)])
                )]))),
                Some(Some(1))
            );

            assert_eq!(
                catalog_handle
                    .work_with(SCHEMA, |schema| schema.work_with(TABLE, |table| table.select()))
                    .unwrap()
                    .unwrap()
                    .collect::<Vec<(Key, Value)>>(),
                vec![
                    (Binary::pack(&[Datum::from_u64(0)]), Binary::pack(&[Datum::from_u64(1)])),
                    (Binary::pack(&[Datum::from_u64(1)]), Binary::pack(&[Datum::from_u64(4)])),
                ]
            );
        }
    }
}
