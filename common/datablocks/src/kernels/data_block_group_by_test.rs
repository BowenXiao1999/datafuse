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

use common_datavalues::prelude::*;
use common_exception::Result;

use crate::*;

#[test]
fn test_data_block_group_by() -> Result<()> {
    let schema = DataSchemaRefExt::create(vec![
        DataField::new("a", DataType::Int8, false),
        DataField::new("b", DataType::String, false),
    ]);

    let block = DataBlock::create_by_array(schema, vec![
        Series::new(vec![1i8, 1, 2, 1, 2, 3]),
        Series::new(vec!["x1", "x1", "x2", "x1", "x2", "x3"]),
    ]);

    let columns = &["a".to_string(), "b".to_string()];
    let table = DataBlock::group_by_blocks(&block, columns)?;
    for block in table {
        match block.num_rows() {
            1 => {
                let expected = vec![
                    "+---+----+",
                    "| a | b  |",
                    "+---+----+",
                    "| 3 | x3 |",
                    "+---+----+",
                ];
                crate::assert_blocks_sorted_eq(expected, &[block]);
            }
            2 => {
                let expected = vec![
                    "+---+----+",
                    "| a | b  |",
                    "+---+----+",
                    "| 2 | x2 |",
                    "| 2 | x2 |",
                    "+---+----+",
                ];
                crate::assert_blocks_sorted_eq(expected, &[block]);
            }
            3 => {
                let expected = vec![
                    "+---+----+",
                    "| a | b  |",
                    "+---+----+",
                    "| 1 | x1 |",
                    "| 1 | x1 |",
                    "| 1 | x1 |",
                    "+---+----+",
                ];
                crate::assert_blocks_sorted_eq(expected, &[block]);
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}
