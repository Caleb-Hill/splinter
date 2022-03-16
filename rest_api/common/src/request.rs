// Copyright 2018-2022 Cargill Incorporated
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

pub trait Request {
    fn get_header_value(&self, _key: &str) -> Option<Vec<u8>> {
        None
    }

    fn get_header_values(&self, _key: &str) -> Box<dyn Iterator<Item = Vec<u8>>>;

    fn get_query_value(&self, _key: &str) -> Option<String>;

    fn uri(&self) -> &str;
}
