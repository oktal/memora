use bytes::{Buf, BufMut, BytesMut};
use logos::Logos;
use tokio_util::codec::{Decoder, Encoder};

use crate::resp::{self, RespError, RespResult};

use super::{error::MemoraError, Response};

pub struct RespFramer;

impl Decoder for RespFramer {
    type Item = resp::Value;
    type Error = RespError;

    fn decode(&mut self, buf: &mut BytesMut) -> RespResult<Option<Self::Item>> {
        let len = buf.len();

        match resp::Value::parse(resp::Token::lexer(&buf)) {
            Ok(Some((value, remainder))) => {
                let parsed_len = len - remainder.len();
                buf.advance(parsed_len);
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl Encoder<resp::Value> for RespFramer {
    type Error = RespError;

    fn encode(&mut self, item: resp::Value, dst: &mut BytesMut) -> RespResult<()> {
        let mut writer = dst.writer();
        item.encode(&mut writer)
    }
}

impl Encoder<Response> for RespFramer {
    type Error = MemoraError;

    fn encode(
        &mut self,
        item: Response,
        dst: &mut BytesMut,
    ) -> std::prelude::v1::Result<(), Self::Error> {
        let mut writer = dst.writer();
        item.encode(&mut writer)
    }
}
