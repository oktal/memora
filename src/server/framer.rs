use bytes::{Buf, BufMut, BytesMut};
use logos::Logos;
use tokio_util::codec::{Decoder, Encoder};

use crate::resp::{self, RespError, RespResult};

use super::{
    error::{MemoraError, MemoraResult},
    Response,
};

pub struct RespFramer;

impl Decoder for RespFramer {
    type Item = resp::Value;
    type Error = MemoraError;

    fn decode(&mut self, buf: &mut BytesMut) -> MemoraResult<Option<Self::Item>> {
        let src = std::str::from_utf8(&buf).map_err(|_| MemoraError::Utf8Error)?;
        let len = src.len();

        match resp::Value::parse(resp::Token::lexer(src)) {
            Ok(Some((value, remainder))) => {
                let parsed_len = len - remainder.len();
                buf.advance(parsed_len);
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(MemoraError::Resp(e)),
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
