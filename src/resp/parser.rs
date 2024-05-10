use logos::Lexer;

use super::{
    lex::Token,
    value::{StringValue, Value},
    RespError, RespResult,
};

pub(super) struct Parser<'a> {
    lexer: Lexer<'a, Token>,
}

impl<'a> Parser<'a> {
    pub fn new(lexer: Lexer<'a, Token>) -> Self {
        Self { lexer }
    }

    /// Parse a RESP bulk string
    /// On success, the outer `Option` indicates whether a string has been parsed
    /// or not. The inner `Option` indicates whether the bulk string is a null string
    fn parse_bulk(&mut self) -> RespResult<Option<Option<String>>> {
        // Read length
        let Some(length) = self.try_next()? else {
            return Ok(None);
        };

        let Some(length) = length.as_int() else {
            return Err(RespError::InvalidToken);
        };

        // Null bulk string
        if length == -1 {
            return Ok(Some(None));
        }

        // This is not a bulk string, attempt to convert the length to a `usize`
        // and error otherwise
        let Ok(length) = length.try_into() else {
            return Err(RespError::InvalidLength(length));
        };

        // Read the string
        let Some(token) = self.try_next()? else {
            return Ok(None);
        };

        let str: String = token.try_into()?;

        // The length of the string we read does not match the expected length,
        // which means that we read a partial string
        if str.len() != length {
            return Ok(None);
        }

        Ok(Some(Some(str.to_owned())))
    }

    /// Attempt to parse a RESP array
    /// On success, return `Some` if a complete array has been parsed or `None`
    /// if a partial array has been parsed
    fn parse_array(&mut self) -> RespResult<Option<Vec<Value>>> {
        // Read length
        let Some(length) = self.try_next()? else {
            return Ok(None);
        };

        let Some(length) = length.as_int() else {
            return Err(RespError::InvalidToken);
        };

        let Ok(length) = length.try_into() else {
            return Err(RespError::InvalidLength(length));
        };

        let values = (0usize..length).map(|_| self.parse_one());
        values.collect()
    }

    pub fn parse(&mut self) -> RespResult<Option<(Value, &'a str)>> {
        match self.parse_one() {
            Ok(Some(value)) => Ok(Some((value, self.lexer.remainder()))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Attempt to parse a RESP value
    /// On success, return `Some` if a complete value has been parsed or `None` if a partial
    /// value was parsed
    pub fn parse_one(&mut self) -> RespResult<Option<Value>> {
        let Some(token) = self.try_next()? else {
            return Ok(None);
        };

        match token {
            Token::Star => {
                let Some(values) = self.parse_array()? else {
                    return Ok(None);
                };
                Ok(Some(Value::Array(values)))
            }
            Token::Dollar => {
                let Some(bulk) = self.parse_bulk()? else {
                    return Ok(None);
                };
                Ok(Some(Value::Str(StringValue::Bulk(bulk))))
            }
            Token::Plus => {
                let str = self.try_next()?;
                let Some(str) = str else {
                    return Ok(None);
                };

                let Token::Str(str) = str else {
                    return Err(RespError::InvalidToken);
                };

                Ok(Some(Value::Str(StringValue::Simple(str))))
            }
            _ => todo!(),
        }
    }

    /// Attempt to consume the next [`Token`]
    /// On success, return `Some` if a token is available or `None` otherwise
    fn try_next(&mut self) -> RespResult<Option<Token>> {
        let Some(token) = self.lexer.next() else {
            return Ok(None);
        };

        token.map(Some).map_err(|_| RespError::InvalidToken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos::Logos;

    #[test]
    fn should_lex() {
        let expected = [
            Token::Star,
            Token::Int(2),
            Token::Dollar,
            Token::Int(4),
            Token::Str("echo".to_string()),
            Token::Dollar,
            Token::Int(3),
            Token::Str("hey".to_string()),
        ];

        let lex = Token::lexer("*2\r\n$4\r\necho\r\n$3\r\nhey\r\n");

        for (expected, tok) in expected.into_iter().zip(lex) {
            let tok = tok.expect(&format!("expected token {:?}", expected));
            assert_eq!(tok, expected);
        }
    }

    #[test]
    fn should_parse() {
        let lex = Token::lexer("*2\r\n$4\r\necho\r\n$3\r\nhey\r\n");
        let mut parser = Parser::new(lex);

        let value = parser
            .parse_one()
            .expect("parse value")
            .expect("parse value");
        assert_eq!(
            value,
            Value::from_iter([Value::bulk("echo"), Value::bulk("hey")])
        );
    }

    #[test]
    fn parse_simple() {
        let lex = Token::lexer("+OK\r\n");
        let mut parser = Parser::new(lex);

        let value = parser
            .parse_one()
            .expect("parse value")
            .expect("parse value");

        assert_eq!(value, Value::simple("OK"))
    }
}
