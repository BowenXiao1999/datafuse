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

use common_runtime::tokio;
use common_tracing::init_tracing_with_file;
use common_tracing::set_panic_hook;
use databend_store::api::HttpService;
use databend_store::api::StoreServer;
use databend_store::configs::Config;
use databend_store::metrics::MetricService;
use log::info;
use metasrv::sled_store::init_sled_db;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conf = Config::from_args();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(conf.log_level.to_lowercase().as_str()),
    )
    .init();

    let _guards = init_tracing_with_file(
        "databend-store",
        conf.log_dir.as_str(),
        conf.log_level.as_str(),
    );
    set_panic_hook();

    info!("{:?}", conf.clone());
    info!(
        "DatabendStore v-{}",
        *databend_store::configs::config::DATABEND_COMMIT_VERSION
    );

    init_sled_db(conf.meta_config.raft_dir.clone());

    // Metric API service.
    {
        let srv = MetricService::create(conf.clone());
        tokio::spawn(async move {
            srv.make_server().expect("Metrics service error");
        });
        info!("Metric API server listening on {}", conf.metric_api_address);
    }

    // HTTP API service.
    {
        let mut srv = HttpService::create(conf.clone());
        info!("HTTP API server listening on {}", conf.http_api_address);
        tokio::spawn(async move {
            srv.start().await.expect("HTTP: admin api error");
        });
    }

    // RPC API service.
    {
        let srv = StoreServer::create(conf.clone());
        info!(
            "DatabendStore API server listening on {}",
            conf.flight_api_address
        );
        let (_stop_tx, fin_rx) = srv.start().await.expect("DatabendStore service error");
        fin_rx.await?;
    }

    Ok(())
}
