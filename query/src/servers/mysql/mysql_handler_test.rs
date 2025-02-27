// Copyright 2020 Datafuse Labs.
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

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Barrier;
use std::thread::JoinHandle;
use std::time::Duration;

use common_exception::ErrorCode;
use common_exception::Result;
use common_exception::ToErrorCode;
use common_runtime::tokio;
use mysql::prelude::FromRow;
use mysql::prelude::Queryable;
use mysql::Conn;
use mysql::FromRowError;
use mysql::Row;

use crate::servers::MySQLHandler;
use crate::tests::try_create_session_mgr;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_use_database_with_on_query() -> Result<()> {
    let mut handler = MySQLHandler::create(try_create_session_mgr(Some(1))?);

    let listening = "0.0.0.0:0".parse::<SocketAddr>()?;
    let runnable_server = handler.start(listening).await?;
    let mut connection = create_connection(runnable_server.port())?;
    let received_data: Vec<String> = query(&mut connection, "SELECT database()")?;
    assert_eq!(received_data, vec!["default"]);
    query::<EmptyRow>(&mut connection, "USE system")?;
    let received_data: Vec<String> = query(&mut connection, "SELECT database()")?;
    assert_eq!(received_data, vec!["system"]);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_rejected_session_with_sequence() -> Result<()> {
    let mut handler = MySQLHandler::create(try_create_session_mgr(Some(1))?);

    let listening = "0.0.0.0:0".parse::<SocketAddr>()?;
    let listening = handler.start(listening).await?;

    {
        // Accepted connection
        let conn = create_connection(listening.port())?;

        // Rejected connection
        match create_connection(listening.port()) {
            Ok(_) => assert!(false, "Expected rejected connection"),
            Err(error) => {
                assert_eq!(error.code(), 1000);
                assert_eq!(error.message(), "Reject connection, cause: MySqlError { ERROR 1203 (42000): The current accept connection has exceeded mysql_handler_thread_num config }");
            }
        };

        drop(conn);
    }

    // Wait for the connection to be destroyed
    std::thread::sleep(Duration::from_secs(5));
    // Accepted connection
    create_connection(listening.port())?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_rejected_session_with_parallel() -> Result<()> {
    enum CreateServerResult {
        Accept,
        Rejected,
    }

    fn connect_server(
        port: u16,
        start_barrier: Arc<Barrier>,
        destroy_barrier: Arc<Barrier>,
    ) -> JoinHandle<CreateServerResult> {
        std::thread::spawn(move || {
            start_barrier.wait();
            match create_connection(port) {
                Ok(_conn) => {
                    destroy_barrier.wait();
                    CreateServerResult::Accept
                }
                Err(error) => {
                    destroy_barrier.wait();
                    assert_eq!(error.code(), 1000);
                    assert_eq!(error.message(), "Reject connection, cause: MySqlError { ERROR 1203 (42000): The current accept connection has exceeded mysql_handler_thread_num config }");
                    CreateServerResult::Rejected
                }
            }
        })
    }

    let mut handler = MySQLHandler::create(try_create_session_mgr(Some(1))?);

    let listening = "0.0.0.0:0".parse::<SocketAddr>()?;
    let listening = handler.start(listening).await?;

    let start_barriers = Arc::new(Barrier::new(3));
    let destroy_barriers = Arc::new(Barrier::new(3));

    let mut join_handlers = Vec::with_capacity(3);
    for _ in 0..3 {
        let start_barrier = start_barriers.clone();
        let destroy_barrier = destroy_barriers.clone();

        join_handlers.push(connect_server(
            listening.port(),
            start_barrier,
            destroy_barrier,
        ));
    }

    let mut accept = 0;
    let mut rejected = 0;
    for join_handler in join_handlers {
        match join_handler.join() {
            Err(error) => assert!(false, "Unexpected error: {:?}", error),
            Ok(CreateServerResult::Accept) => accept += 1,
            Ok(CreateServerResult::Rejected) => rejected += 1,
        }
    }

    assert_eq!(accept, 1);
    assert_eq!(rejected, 2);

    Ok(())
}

fn query<T: FromRow>(connection: &mut Conn, query: &str) -> Result<Vec<T>> {
    connection
        .query::<T, &str>(query)
        .map_err_to_code(ErrorCode::UnknownException, || "Query error")
}

fn create_connection(port: u16) -> Result<mysql::Conn> {
    let uri = &format!("mysql://127.0.0.1:{}?user=default", port);
    let opts = mysql::Opts::from_url(uri).unwrap();
    mysql::Conn::new(opts).map_err_to_code(ErrorCode::UnknownException, || "Reject connection")
}

struct EmptyRow;

impl FromRow for EmptyRow {
    fn from_row_opt(_: Row) -> std::result::Result<Self, FromRowError>
    where Self: Sized {
        Ok(EmptyRow)
    }
}
