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

use actix_web::web::Payload;

use splinter::protobuf::Message;

pub fn into_protobuf<M: Message>(payload: Payload) -> impl Future<Item = Result<M, ActixError>> {
    payload
        .from_err::<ActixError>()
        .fold(web::BytesMut::new(), move |mut body, chunk| {
            body.extend_from_slice(&chunk);
            Ok::<_, ActixError>(body)
        })
        .and_then(|body| match Message::parse_from_bytes(&body) {
            Ok(proto) => Ok(proto),
            Err(err) => Err(ErrorBadRequest(json!({ "message": format!("{}", err) }))),
        })
        .into_future()
}
