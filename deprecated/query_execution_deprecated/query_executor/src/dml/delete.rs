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

use connection::Sender;
use data_manager::DatabaseHandle;
use pg_model::results::QueryEvent;
use plan::TableDeletes;
use std::sync::Arc;

pub(crate) struct DeleteCommand {
    table_deletes: TableDeletes,
    data_manager: Arc<DatabaseHandle>,
    sender: Arc<dyn Sender>,
}

impl DeleteCommand {
    pub(crate) fn new(
        table_deletes: TableDeletes,
        data_manager: Arc<DatabaseHandle>,
        sender: Arc<dyn Sender>,
    ) -> DeleteCommand {
        DeleteCommand {
            table_deletes,
            data_manager,
            sender,
        }
    }

    pub(crate) fn execute(&self) {
        let reads = match self.data_manager.full_scan(&self.table_deletes.table_id) {
            Err(()) => {
                log::error!("Error while scanning {:?}", self.table_deletes.table_id);
                return;
            }
            Ok(reads) => reads,
        };
        let keys = reads
            .map(Result::unwrap)
            .map(Result::unwrap)
            .map(|(key, _)| key)
            .collect();

        let size = match self.data_manager.delete_from(&self.table_deletes.table_id, keys) {
            Err(()) => {
                log::error!("Error while deleting from {:?}", self.table_deletes.table_id);
                return;
            }
            Ok(size) => size,
        };
        self.sender
            .send(Ok(QueryEvent::RecordsDeleted(size)))
            .expect("To Send Query Result to Client");
    }
}
