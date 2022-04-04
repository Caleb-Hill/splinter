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

use actix_web::web::{BytesMut,Bytes };
use actix_web::dev::Payload;

use futures::{Future, TryFutureExt, TryStreamExt,FutureExt};
use protobuf::Message;

use crate::error::RestError;

pub fn payload_bytes(payload: Payload) -> impl Future<Output = Result<Bytes,RestError>> {
    payload
        .try_fold(BytesMut::new(), |mut body, chunk| async move {
            body.extend_from_slice(&chunk);
            Ok(body)
        })
        .map_err(|_| RestError::BadRequest("bad protobuf".to_string()))
        .map_ok(|body| body.freeze())
}

pub fn bytes_into_protobuf<M: Message>(body: &[u8]) -> Result<M,RestError> {
    Message::parse_from_bytes(body)
                .map_err(|_| RestError::BadRequest("bad protobuf".to_string()))

}

pub fn into_protobuf<M: Message>(payload:Payload) -> impl Future< Output = Result<M,RestError>> {
    payload_bytes(payload)
        .map(|result| { match result {
            Ok(body) =>bytes_into_protobuf::<M>(&body),
            Err(err) => Err(err)
        }
        })
}
