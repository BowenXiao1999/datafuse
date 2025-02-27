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
//

use common_exception::Result;
use common_runtime::tokio;

use crate::DNSResolver;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_resolver_github() -> Result<()> {
    let addrs = DNSResolver::instance()?.resolve("github.com").await?;
    assert_ne!(addrs.len(), 0);

    Ok(())
}
