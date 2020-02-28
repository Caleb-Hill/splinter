// Copyright 2018-2020 Cargill Incorporated
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

#[derive(Debug)]
pub struct CircuitTemplateError {
    context: String,
    source: Option<Box<dyn std::error::Error>>,
}

impl CircuitTemplateError {
    pub fn new(context: &str) -> Self {
        Self {
            context: context.into(),
            source: None,
        }
    }

    pub fn new_with_source(context: &str, err: Box<dyn std::error::Error>) -> Self {
        Self {
            context: context.into(),
            source: Some(err),
        }
    }
}

impl std::error::Error for CircuitTemplateError {}

impl std::fmt::Display for CircuitTemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if let Some(ref err) = self.source {
            write!(f, "{}: {}", self.context, err)
        } else {
            f.write_str(&self.context)
        }
    }
}

impl From<serde_yaml::Error> for CircuitTemplateError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::new_with_source("Error deserializing template", err.into())
    }
}
