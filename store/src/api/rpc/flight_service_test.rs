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

use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use common_datablocks::DataBlock;
use common_datavalues::prelude::*;
use common_metatypes::KVMeta;
use common_metatypes::KVValue;
use common_metatypes::MatchSeq;
use common_planners::CreateDatabasePlan;
use common_planners::CreateTablePlan;
use common_planners::DropDatabasePlan;
use common_planners::DropTablePlan;
use common_planners::ScanPlan;
use common_runtime::tokio;
use common_store_api_sdk::meta_api_impl::DropTableActionResult;
use common_store_api_sdk::meta_api_impl::GetTableActionResult;
use common_store_api_sdk::KVApi;
use common_store_api_sdk::MetaApi;
use common_store_api_sdk::StorageApi;
use common_store_api_sdk::StoreClient;
use common_tracing::tracing;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_restart() -> anyhow::Result<()> {
    // Issue 1134  https://github.com/datafuselabs/databend/issues/1134
    // - Start a store server.
    // - create db and create table
    // - restart
    // - Test read the db and read the table.

    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();

    let (mut tc, addr) = crate::tests::start_store_server().await?;

    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    let db_name = "db1";
    let table_name = "table1";

    tracing::info!("--- create db");
    {
        let plan = CreateDatabasePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };

        let res = client.create_database(plan.clone()).await;
        tracing::debug!("create database res: {:?}", res);
        let res = res?;
        assert_eq!(1, res.database_id, "first database id is 1");
    }

    tracing::info!("--- get db");
    {
        let res = client.get_database(db_name).await;
        tracing::debug!("get present database res: {:?}", res);
        let res = res?;
        assert_eq!(1, res.database_id, "db1 id is 1");
        assert_eq!(db_name, res.db, "db1.db is db1");
    }

    tracing::info!("--- create table {}.{}", db_name, table_name);
    let schema = Arc::new(DataSchema::new(vec![DataField::new(
        "number",
        DataType::UInt64,
        false,
    )]));
    {
        let options = maplit::hashmap! {"opt‐1".into() => "val-1".into()};
        let plan = CreateTablePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            table: table_name.to_string(),
            schema: schema.clone(),
            options: options.clone(),
            engine: "JSON".to_string(),
        };

        {
            let res = client.create_table(plan.clone()).await?;
            assert_eq!(1, res.table_id, "table id is 1");

            let got = client.get_table(db_name.into(), table_name.into()).await?;
            let want = GetTableActionResult {
                table_id: 1,
                db: db_name.into(),
                name: table_name.into(),
                schema: schema.clone(),
                engine: "JSON".to_owned(),
                options: options.clone(),
            };
            assert_eq!(want, got, "get created table");
        }
    }

    tracing::info!("--- stop StoreServer");
    {
        let (stop_tx, fin_rx) = tc.channels.take().unwrap();
        stop_tx
            .send(())
            .map_err(|_| anyhow::anyhow!("fail to send"))?;

        fin_rx.await?;

        drop(client);

        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // restart by opening existent meta db
        tc.config.meta_config.boot = false;
        crate::tests::start_store_server_with_context(&mut tc).await?;
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(10_000)).await;

    // try to reconnect the restarted server.
    let mut _client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    // TODO(xp): db and table are still in pure memory store. the following test will no pass.

    // tracing::info!("--- get db");
    // {
    //     let res = client.get_database(db_name).await;
    //     tracing::debug!("get present database res: {:?}", res);
    //     let res = res?;
    //     assert_eq!(1, res.database_id, "db1 id is 1");
    //     assert_eq!(db_name, res.db, "db1.db is db1");
    // }
    //
    // tracing::info!("--- get table");
    // {
    //     let got = client
    //         .get_table(db_name.into(), table_name.into())
    //         .await
    //         .unwrap();
    //     let want = GetTableActionResult {
    //         table_id: 1,
    //         db: db_name.into(),
    //         name: table_name.into(),
    //         schema: schema.clone(),
    //     };
    //     assert_eq!(want, got, "get created table");
    // }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_create_database() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();

    // 1. Service starts.
    let (_tc, addr) = crate::tests::start_store_server().await?;

    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    // 2. Create database.

    // TODO: test arg if_not_exists: It should respond  an ErrorCode
    {
        // create first db
        let plan = CreateDatabasePlan {
            // TODO test if_not_exists
            if_not_exists: false,
            db: "db1".to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };

        let res = client.create_database(plan.clone()).await;
        tracing::info!("create database res: {:?}", res);
        let res = res.unwrap();
        assert_eq!(1, res.database_id, "first database id is 1");
    }
    {
        // create second db
        let plan = CreateDatabasePlan {
            if_not_exists: false,
            db: "db2".to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };

        let res = client.create_database(plan.clone()).await;
        tracing::info!("create database res: {:?}", res);
        let res = res.unwrap();
        assert_eq!(2, res.database_id, "second database id is 2");
    }

    // 3. Get database.

    {
        // get present db
        let res = client.get_database("db1").await;
        tracing::debug!("get present database res: {:?}", res);
        let res = res?;
        assert_eq!(1, res.database_id, "db1 id is 1");
        assert_eq!("db1".to_string(), res.db, "db1.db is db1");
    }

    {
        // get absent db
        let res = client.get_database("ghost").await;
        tracing::debug!("=== get absent database res: {:?}", res);
        assert!(res.is_err());
        let res = res.unwrap_err();
        assert_eq!(3, res.code());
        assert_eq!("ghost".to_string(), res.message());
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_create_get_table() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    use std::sync::Arc;

    use common_datavalues::DataField;
    use common_datavalues::DataSchema;
    use common_planners::CreateDatabasePlan;
    use common_planners::CreateTablePlan;

    tracing::info!("init logging");

    // 1. Service starts.
    let (_tc, addr) = crate::tests::start_store_server().await?;

    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    let db_name = "db1";
    let tbl_name = "tb2";

    {
        // prepare db
        let plan = CreateDatabasePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };

        let res = client.create_database(plan.clone()).await;

        tracing::info!("create database res: {:?}", res);

        let res = res.unwrap();
        assert_eq!(1, res.database_id, "first database id is 1");
    }
    {
        // create table and fetch it

        // Table schema with metadata(due to serde issue).
        let schema = Arc::new(DataSchema::new(vec![DataField::new(
            "number",
            DataType::UInt64,
            false,
        )]));

        let options = maplit::hashmap! {"opt‐1".into() => "val-1".into()};
        // Create table plan.
        let mut plan = CreateTablePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            table: tbl_name.to_string(),
            schema: schema.clone(),
            options: options.clone(),
            engine: "JSON".to_string(),
        };

        {
            // create table OK
            let res = client.create_table(plan.clone()).await.unwrap();
            assert_eq!(1, res.table_id, "table id is 1");

            let got = client
                .get_table(db_name.into(), tbl_name.into())
                .await
                .unwrap();
            let want = GetTableActionResult {
                table_id: 1,
                db: db_name.into(),
                name: tbl_name.into(),
                schema: schema.clone(),
                engine: "JSON".to_owned(),
                options: options.clone(),
            };
            assert_eq!(want, got, "get created table");
        }

        {
            // create table again with if_not_exists = true
            plan.if_not_exists = true;
            let res = client.create_table(plan.clone()).await.unwrap();
            assert_eq!(1, res.table_id, "new table id");

            let got = client
                .get_table(db_name.into(), tbl_name.into())
                .await
                .unwrap();
            let want = GetTableActionResult {
                table_id: 1,
                db: db_name.into(),
                name: tbl_name.into(),
                schema: schema.clone(),
                engine: "JSON".to_owned(),
                options: options.clone(),
            };
            assert_eq!(want, got, "get created table");
        }

        {
            // create table again with if_not_exists=false
            plan.if_not_exists = false;

            let res = client.create_table(plan.clone()).await;
            tracing::info!("create table res: {:?}", res);

            let status = res.err().unwrap();
            assert_eq!(
                format!("Code: 4003, displayText = table exists: {}.", tbl_name),
                status.to_string()
            );

            // get_table returns the old table

            let got = client.get_table("db1".into(), "tb2".into()).await.unwrap();
            let want = GetTableActionResult {
                table_id: 1,
                db: db_name.into(),
                name: tbl_name.into(),
                schema: schema.clone(),
                engine: "JSON".to_owned(),
                options: options.clone(),
            };
            assert_eq!(want, got, "get old table");
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_drop_table() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    use std::sync::Arc;

    use common_datavalues::DataField;
    use common_datavalues::DataSchema;
    use common_planners::CreateDatabasePlan;
    use common_planners::CreateTablePlan;

    tracing::info!("init logging");

    // 1. Service starts.
    let (_tc, addr) = crate::tests::start_store_server().await?;

    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    let db_name = "db1";
    let tbl_name = "tb2";

    {
        // prepare db
        let plan = CreateDatabasePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };

        let res = client.create_database(plan.clone()).await;

        tracing::info!("create database res: {:?}", res);

        let res = res.unwrap();
        assert_eq!(1, res.database_id, "first database id is 1");
    }
    {
        // create table and fetch it

        // Table schema with metadata(due to serde issue).
        let schema = Arc::new(DataSchema::new(vec![DataField::new(
            "number",
            DataType::UInt64,
            false,
        )]));

        let options = maplit::hashmap! {"opt‐1".into() => "val-1".into()};
        // Create table plan.
        let plan = CreateTablePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            table: tbl_name.to_string(),
            schema: schema.clone(),
            options: options.clone(),
            engine: "JSON".to_string(),
        };

        {
            // create table OK
            let res = client.create_table(plan.clone()).await.unwrap();
            assert_eq!(1, res.table_id, "table id is 1");

            let got = client
                .get_table(db_name.into(), tbl_name.into())
                .await
                .unwrap();
            let want = GetTableActionResult {
                table_id: 1,
                db: db_name.into(),
                name: tbl_name.into(),
                schema: schema.clone(),
                engine: "JSON".to_owned(),
                options: options.clone(),
            };
            assert_eq!(want, got, "get created table");
        }

        {
            // drop table
            let plan = DropTablePlan {
                if_exists: true,
                db: db_name.to_string(),
                table: tbl_name.to_string(),
            };
            let res = client.drop_table(plan.clone()).await.unwrap();
            assert_eq!(DropTableActionResult {}, res, "drop table {}", tbl_name)
        }

        {
            let res = client.get_table(db_name.into(), tbl_name.into()).await;
            let status = res.err().unwrap();
            assert_eq!(
                format!("Code: 25, displayText = table not found: {}.", tbl_name),
                status.to_string(),
                "get dropped table {}",
                tbl_name
            );
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_do_append() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();

    use std::sync::Arc;

    use common_datavalues::prelude::*;
    use common_planners::CreateDatabasePlan;
    use common_planners::CreateTablePlan;

    let (_tc, addr) = crate::tests::start_store_server().await?;

    let schema = Arc::new(DataSchema::new(vec![
        DataField::new("col_i", DataType::Int64, false),
        DataField::new("col_s", DataType::String, false),
    ]));
    let db_name = "test_db";
    let tbl_name = "test_tbl";

    let series0 = Series::new(vec![0i64, 1, 2]);
    let series1 = Series::new(vec!["str1", "str2", "str3"]);

    let expected_rows = series0.len() * 2;
    let expected_cols = 2;

    let block = DataBlock::create_by_array(schema.clone(), vec![series0, series1]);
    let batches = vec![block.clone(), block];
    let num_batch = batches.len();
    let stream = futures::stream::iter(batches);

    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;
    {
        let plan = CreateDatabasePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };
        let res = client.create_database(plan.clone()).await;
        let res = res.unwrap();
        assert_eq!(res.database_id, 1, "db created");
        let plan = CreateTablePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            table: tbl_name.to_string(),
            schema: schema.clone(),
            options: maplit::hashmap! {"opt‐1".into() => "val-1".into()},
            engine: "PARQUET".to_string(),
        };
        client.create_table(plan.clone()).await.unwrap();
    }
    let res = client
        .append_data(
            db_name.to_string(),
            tbl_name.to_string(),
            schema,
            Box::pin(stream),
        )
        .await
        .unwrap();
    tracing::info!("append res is {:?}", res);
    let summary = res.summary;
    assert_eq!(summary.rows, expected_rows, "rows eq");
    assert_eq!(res.parts.len(), num_batch, "batch eq");
    res.parts.iter().for_each(|p| {
        assert_eq!(p.rows, expected_rows / num_batch);
        assert_eq!(p.cols, expected_cols);
    });
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_scan_partition() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    use std::sync::Arc;

    use common_datavalues::prelude::*;
    use common_planners::CreateDatabasePlan;
    use common_planners::CreateTablePlan;

    let (_tc, addr) = crate::tests::start_store_server().await?;

    let schema = Arc::new(DataSchema::new(vec![
        DataField::new("col_i", DataType::Int64, false),
        DataField::new("col_s", DataType::String, false),
    ]));
    let db_name = "test_db";
    let tbl_name = "test_tbl";

    let series0 = Series::new(vec![0i64, 1, 2]);
    let series1 = Series::new(vec!["str1", "str2", "str3"]);

    let rows_of_series0 = series0.len();
    let rows_of_series1 = series1.len();
    let expected_rows = rows_of_series0 + rows_of_series1;
    let expected_cols = 2;

    let block = DataBlock::create(schema.clone(), vec![
        DataColumn::Array(series0),
        DataColumn::Array(series1),
    ]);
    let batches = vec![block.clone(), block];
    let num_batch = batches.len();
    let stream = futures::stream::iter(batches);

    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;
    {
        let plan = CreateDatabasePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            engine: "Local".to_string(),
            options: Default::default(),
        };
        client.create_database(plan.clone()).await?;
        let plan = CreateTablePlan {
            if_not_exists: false,
            db: db_name.to_string(),
            table: tbl_name.to_string(),
            schema: schema.clone(),
            options: maplit::hashmap! {"opt‐1".into() => "val-1".into()},
            engine: "PARQUET".to_string(),
        };
        client.create_table(plan.clone()).await?;
    }
    let res = client
        .append_data(
            db_name.to_string(),
            tbl_name.to_string(),
            schema,
            Box::pin(stream),
        )
        .await?;
    tracing::info!("append res is {:?}", res);
    let summary = res.summary;
    assert_eq!(summary.rows, expected_rows);
    assert_eq!(res.parts.len(), num_batch);
    res.parts.iter().for_each(|p| {
        assert_eq!(p.rows, expected_rows / num_batch);
        assert_eq!(p.cols, expected_cols);
    });

    log::debug!("summary is {:?}", summary);

    let plan = ScanPlan {
        schema_name: tbl_name.to_string(),
        ..ScanPlan::empty()
    };
    let res = client
        .read_plan(db_name.to_string(), tbl_name.to_string(), &plan)
        .await;

    assert!(res.is_ok());
    let read_plan_res = res.unwrap();
    assert!(read_plan_res.is_some());
    let read_plan = read_plan_res.unwrap();
    assert_eq!(2, read_plan.len());
    assert_eq!(read_plan[0].stats.read_rows, rows_of_series0);
    assert_eq!(read_plan[1].stats.read_rows, rows_of_series1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv_mget() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv_list");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        client
            .upsert_kv("k1", MatchSeq::Any, Some(b"v1".to_vec()), None)
            .await?;
        client
            .upsert_kv("k2", MatchSeq::Any, Some(b"v2".to_vec()), None)
            .await?;

        let res = client
            .mget_kv(&["k1".to_string(), "k2".to_string()])
            .await?;
        assert_eq!(res.result, vec![
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            // NOTE, the sequence number is increased globally (inside the namespace of generic kv)
            Some((2, KVValue {
                meta: None,
                value: b"v2".to_vec()
            })),
        ]);

        let res = client
            .mget_kv(&["k1".to_string(), "key_no exist".to_string()])
            .await?;
        assert_eq!(res.result, vec![
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            None
        ]);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv_list() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv_list");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        let mut values = vec![];
        {
            client
                .upsert_kv("t", MatchSeq::Any, Some("".as_bytes().to_vec()), None)
                .await?;

            for i in 0..9 {
                let key = format!("__users/{}", i);
                let val = format!("val_{}", i);
                values.push(val.clone());
                client
                    .upsert_kv(&key, MatchSeq::Any, Some(val.as_bytes().to_vec()), None)
                    .await?;
            }
            client
                .upsert_kv("v", MatchSeq::Any, Some(b"".to_vec()), None)
                .await?;
        }

        let res = client.prefix_list_kv("__users/").await?;
        assert_eq!(
            res.iter()
                .map(|(_key, (_s, val))| val.clone())
                .collect::<Vec<_>>(),
            values
                .iter()
                .map(|v| KVValue {
                    meta: None,
                    value: v.as_bytes().to_vec()
                })
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv_delete() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv_list");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        let test_key = "test_key";
        client
            .upsert_kv(test_key, MatchSeq::Any, Some(b"v1".to_vec()), None)
            .await?;

        let current = client.get_kv(test_key).await?;
        if let Some((seq, _val)) = current.result {
            // seq mismatch
            let wrong_seq = Some(seq + 1);
            let res = client
                .upsert_kv(test_key, wrong_seq.into(), None, None)
                .await?;
            assert_eq!(res.prev, res.result);

            // seq match
            let res = client
                .upsert_kv(test_key, MatchSeq::Exact(seq), None, None)
                .await?;
            assert!(res.result.is_none());

            // read nothing
            let r = client.get_kv(test_key).await?;
            assert!(r.result.is_none());
        } else {
            panic!("expecting a value, but got nothing");
        }

        // key not exist
        let res = client
            .upsert_kv("not exists", MatchSeq::Any, None, None)
            .await?;
        assert_eq!(None, res.prev);
        assert_eq!(None, res.result);

        // do not care seq
        client
            .upsert_kv(test_key, MatchSeq::Any, Some(b"v2".to_vec()), None)
            .await?;

        let res = client
            .upsert_kv(test_key, MatchSeq::Any, None, None)
            .await?;
        assert_eq!(
            (
                Some((2, KVValue {
                    meta: None,
                    value: b"v2".to_vec()
                })),
                None
            ),
            (res.prev, res.result)
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv_update() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv_list");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        let test_key = "test_key_for_update";

        let r = client
            .upsert_kv(test_key, MatchSeq::GE(1), Some(b"v1".to_vec()), None)
            .await?;
        assert_eq!((None, None), (r.prev, r.result), "not changed");

        let r = client
            .upsert_kv(test_key, MatchSeq::Any, Some(b"v1".to_vec()), None)
            .await?;
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.result
        );
        let seq = r.result.unwrap().0;

        // unmatched seq
        let r = client
            .upsert_kv(
                test_key,
                MatchSeq::Exact(seq + 1),
                Some(b"v2".to_vec()),
                None,
            )
            .await?;
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.prev
        );
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.result
        );

        // matched seq
        let r = client
            .upsert_kv(test_key, MatchSeq::Exact(seq), Some(b"v2".to_vec()), None)
            .await?;
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.prev
        );
        assert_eq!(
            Some((2, KVValue {
                meta: None,
                value: b"v2".to_vec()
            })),
            r.result
        );

        // blind update
        let r = client
            .upsert_kv(test_key, MatchSeq::GE(1), Some(b"v3".to_vec()), None)
            .await?;
        assert_eq!(
            Some((2, KVValue {
                meta: None,
                value: b"v2".to_vec()
            })),
            r.prev
        );
        assert_eq!(
            Some((3, KVValue {
                meta: None,
                value: b"v3".to_vec()
            })),
            r.result
        );

        // value updated
        let kv = client.get_kv(test_key).await?;
        assert!(kv.result.is_some());
        assert_eq!(kv.result.unwrap().1, KVValue {
            meta: None,
            value: b"v3".to_vec()
        });
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv_update_meta() -> anyhow::Result<()> {
    // Only update meta, do not touch the value part.

    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv_update_meta");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        let test_key = "test_key_for_update_meta";

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let r = client
            .upsert_kv(test_key, MatchSeq::Any, Some(b"v1".to_vec()), None)
            .await?;
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.result
        );
        let seq = r.result.unwrap().0;

        tracing::info!("--- mismatching seq does nothing");

        let r = client
            .update_kv_meta(
                test_key,
                MatchSeq::Exact(seq + 1),
                Some(KVMeta {
                    expire_at: Some(now + 20),
                }),
            )
            .await?;
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.prev
        );
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.result
        );

        tracing::info!("--- matching seq only update meta");

        let r = client
            .update_kv_meta(
                test_key,
                MatchSeq::Exact(seq),
                Some(KVMeta {
                    expire_at: Some(now + 20),
                }),
            )
            .await?;
        assert_eq!(
            Some((1, KVValue {
                meta: None,
                value: b"v1".to_vec()
            })),
            r.prev
        );
        assert_eq!(
            Some((2, KVValue {
                meta: Some(KVMeta {
                    expire_at: Some(now + 20)
                }),
                value: b"v1".to_vec()
            })),
            r.result
        );

        tracing::info!("--- get returns the value with meta and seq updated");
        let kv = client.get_kv(test_key).await?;
        assert!(kv.result.is_some());
        assert_eq!(
            (seq + 1, KVValue {
                meta: Some(KVMeta {
                    expire_at: Some(now + 20)
                }),
                value: b"v1".to_vec()
            }),
            kv.result.unwrap(),
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv_timeout() -> anyhow::Result<()> {
    // - Test get  expired and non-expired.
    // - Test mget expired and non-expired.
    // - Test list expired and non-expired.
    // - Test update with a new expire value.

    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv_timeout");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        client
            .upsert_kv(
                "k1",
                MatchSeq::Any,
                Some(b"v1".to_vec()),
                Some(KVMeta {
                    expire_at: Some(now + 1),
                }),
            )
            .await?;

        tracing::info!("---get unexpired");
        {
            let res = client.get_kv(&"k1".to_string()).await?;
            assert!(res.result.is_some(), "got unexpired");
        }

        tracing::info!("---get expired");
        {
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
            let res = client.get_kv(&"k1".to_string()).await?;
            tracing::debug!("got k1:{:?}", res);
            assert!(res.result.is_none(), "got expired");
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        tracing::info!("--- expired entry act as if it does not exist, an ADD op should apply");
        {
            client
                .upsert_kv(
                    "k1",
                    MatchSeq::Exact(0),
                    Some(b"v1".to_vec()),
                    Some(KVMeta {
                        expire_at: Some(now - 1),
                    }),
                )
                .await?;
            client
                .upsert_kv(
                    "k2",
                    MatchSeq::Exact(0),
                    Some(b"v2".to_vec()),
                    Some(KVMeta {
                        expire_at: Some(now + 2),
                    }),
                )
                .await?;

            tracing::info!("--- mget should not return expired");
            let res = client
                .mget_kv(&["k1".to_string(), "k2".to_string()])
                .await?;
            assert_eq!(res.result, vec![
                None,
                Some((3, KVValue {
                    meta: Some(KVMeta {
                        expire_at: Some(now + 2)
                    }),
                    value: b"v2".to_vec()
                })),
            ]);
        }

        tracing::info!("--- list should not return expired");
        {
            let res = client.prefix_list_kv("k").await?;
            let res_vec = res.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();

            assert_eq!(res_vec, vec!["k2".to_string(),]);
        }

        tracing::info!("--- update expire");
        {
            client
                .upsert_kv(
                    "k2",
                    MatchSeq::Exact(3),
                    Some(b"v2".to_vec()),
                    Some(KVMeta {
                        expire_at: Some(now - 1),
                    }),
                )
                .await?;

            let res = client.get_kv(&"k2".to_string()).await?;
            assert!(res.result.is_none(), "k2 expired");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_generic_kv() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();

    {
        let span = tracing::span!(tracing::Level::INFO, "test_flight_generic_kv");
        let _ent = span.enter();

        let (_tc, addr) = crate::tests::start_store_server().await?;

        let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

        {
            // write
            let res = client
                .upsert_kv("foo", MatchSeq::Any, Some(b"bar".to_vec()), None)
                .await?;
            assert_eq!(None, res.prev);
            assert_eq!(
                Some((1, KVValue {
                    meta: None,
                    value: b"bar".to_vec()
                })),
                res.result
            );
        }

        {
            // write fails with unmatched seq
            let res = client
                .upsert_kv("foo", MatchSeq::Exact(2), Some(b"bar".to_vec()), None)
                .await?;
            assert_eq!(
                (
                    Some((1, KVValue {
                        meta: None,
                        value: b"bar".to_vec()
                    })),
                    Some((1, KVValue {
                        meta: None,
                        value: b"bar".to_vec(),
                    })),
                ),
                (res.prev, res.result),
                "nothing changed"
            );
        }

        {
            // write done with matching seq
            let res = client
                .upsert_kv("foo", MatchSeq::Exact(1), Some(b"wow".to_vec()), None)
                .await?;
            assert_eq!(
                Some((1, KVValue {
                    meta: None,
                    value: b"bar".to_vec()
                })),
                res.prev,
                "old value"
            );
            assert_eq!(
                Some((2, KVValue {
                    meta: None,
                    value: b"wow".to_vec()
                })),
                res.result,
                "new value"
            );
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_get_database_meta_empty_db() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    let (_tc, addr) = crate::tests::start_store_server().await?;
    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    // Empty Database
    let res = client.get_database_meta(None).await?;
    assert!(res.is_none());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_get_database_meta_ddl_db() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    let (_tc, addr) = crate::tests::start_store_server().await?;
    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    // create-db operation will increases meta_version
    let plan = CreateDatabasePlan {
        if_not_exists: false,
        db: "db1".to_string(),
        engine: "Local".to_string(),
        options: Default::default(),
    };
    client.create_database(plan).await?;

    let res = client.get_database_meta(None).await?;
    assert!(res.is_some());
    let snapshot = res.unwrap();
    assert_eq!(1, snapshot.meta_ver);
    assert_eq!(1, snapshot.db_metas.len());

    // if lower_bound < current meta version, returns database meta
    let res = client.get_database_meta(Some(0)).await?;
    assert!(res.is_some());
    let snapshot = res.unwrap();
    assert_eq!(1, snapshot.meta_ver);
    assert_eq!(1, snapshot.db_metas.len());

    // if lower_bound equals current meta version, returns None
    let res = client.get_database_meta(Some(1)).await?;
    assert!(res.is_none());

    // failed ddl do not effect meta version
    let plan = CreateDatabasePlan {
        if_not_exists: true, // <<--
        db: "db1".to_string(),
        engine: "Local".to_string(),
        options: Default::default(),
    };

    client.create_database(plan).await?;
    let res = client.get_database_meta(Some(1)).await?;
    assert!(res.is_none());

    // drop-db will increase meta version
    let plan = DropDatabasePlan {
        if_exists: true,
        db: "db1".to_string(),
    };

    client.drop_database(plan).await?;
    let res = client.get_database_meta(Some(1)).await?;
    assert!(res.is_some());
    let snapshot = res.unwrap();

    assert_eq!(2, snapshot.meta_ver);
    assert_eq!(0, snapshot.db_metas.len());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_flight_get_database_meta_ddl_table() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_store_ut!();
    let _ent = ut_span.enter();
    let (_tc, addr) = crate::tests::start_store_server().await?;
    let client = StoreClient::try_create(addr.as_str(), "root", "xxx").await?;

    let test_db = "db1";
    let plan = CreateDatabasePlan {
        if_not_exists: false,
        db: test_db.to_string(),
        engine: "Local".to_string(),
        options: Default::default(),
    };
    client.create_database(plan).await?;

    // After `create db`, meta_ver will be increased to 1

    let schema = Arc::new(DataSchema::new(vec![DataField::new(
        "number",
        DataType::UInt64,
        false,
    )]));

    // create-tbl operation will increases meta_version
    let plan = CreateTablePlan {
        if_not_exists: true,
        db: test_db.to_string(),
        table: "tbl1".to_string(),
        schema: schema.clone(),
        options: Default::default(),
        engine: "JSON".to_string(),
    };

    client.create_table(plan.clone()).await?;

    let res = client.get_database_meta(None).await?;
    assert!(res.is_some());
    let snapshot = res.unwrap();
    assert_eq!(2, snapshot.meta_ver);
    assert_eq!(1, snapshot.db_metas.len());
    assert_eq!(1, snapshot.tbl_metas.len());

    // if lower_bound < current meta version, returns database meta
    let res = client.get_database_meta(Some(0)).await?;
    assert!(res.is_some());
    let snapshot = res.unwrap();
    assert_eq!(2, snapshot.meta_ver);
    assert_eq!(1, snapshot.db_metas.len());

    // if lower_bound equals current meta version, returns None
    let res = client.get_database_meta(Some(2)).await?;
    assert!(res.is_none());

    // failed ddl do not effect meta version
    //  recall: plan.if_not_exist == true
    let _r = client.create_table(plan).await?;
    let res = client.get_database_meta(Some(2)).await?;
    assert!(res.is_none());

    // drop-table will increase meta version
    let plan = DropTablePlan {
        if_exists: true,
        db: test_db.to_string(),
        table: "tbl1".to_string(),
    };

    client.drop_table(plan).await?;
    let res = client.get_database_meta(Some(2)).await?;
    assert!(res.is_some());
    let snapshot = res.unwrap();
    assert_eq!(3, snapshot.meta_ver);
    assert_eq!(1, snapshot.db_metas.len());
    assert_eq!(0, snapshot.tbl_metas.len());

    Ok(())
}
