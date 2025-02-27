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

use common_datavalues::DataSchema;
use common_exception::ErrorCode;
use common_exception::Result;
use common_planners::KillPlan;
use common_streams::DataBlockStream;
use common_streams::SendableDataBlockStream;

use crate::interpreters::Interpreter;
use crate::interpreters::InterpreterPtr;
use crate::sessions::DatabendQueryContextRef;

pub struct KillInterpreter {
    ctx: DatabendQueryContextRef,
    plan: KillPlan,
}

impl KillInterpreter {
    pub fn try_create(ctx: DatabendQueryContextRef, plan: KillPlan) -> Result<InterpreterPtr> {
        Ok(Arc::new(KillInterpreter { ctx, plan }))
    }
}

#[async_trait::async_trait]
impl Interpreter for KillInterpreter {
    fn name(&self) -> &str {
        "KillInterpreter"
    }

    async fn execute(&self) -> Result<SendableDataBlockStream> {
        let id = &self.plan.id;
        match self.ctx.get_sessions_manager().get_session(id) {
            None => Err(ErrorCode::UnknownSession(format!(
                "Not found session id {}",
                id
            ))),
            Some(kill_session) if self.plan.kill_connection => {
                kill_session.force_kill_session();
                let schema = Arc::new(DataSchema::empty());
                Ok(Box::pin(DataBlockStream::create(schema, None, vec![])))
            }
            Some(kill_session) => {
                kill_session.force_kill_query();
                let schema = Arc::new(DataSchema::empty());
                Ok(Box::pin(DataBlockStream::create(schema, None, vec![])))
            }
        }
    }
}
