//! The Rust prompt consumes DOS-encoded bytes. Double quotes group a word and
//! protect operators; ECHO also needs the original spelling and whitespace.

pub(super) struct CommandLine<'a> {
    pub name: Vec<u8>,
    pub arguments: Vec<Word<'a>>,
    pub stdout: Option<Vec<u8>>,
    trailing_space: &'a [u8],
}

pub(super) struct Word<'a> {
    pub value: Vec<u8>,
    spelling: &'a [u8],
    preceding_space: &'a [u8],
}

enum Token<'a> {
    Word(Word<'a>),
    RedirectOutput,
}

struct Lexer<'a> {
    rest: &'a [u8],
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Option<Token<'a>>, &'static str> {
        let input = self.rest.trim_ascii_start();
        let preceding_space = &self.rest[..self.rest.len() - input.len()];
        match input {
            [] => Ok(None),
            [b'>', b'>', rest @ ..] | [b'>', rest @ ..] => {
                // Both > and >> open the same stream for character devices.
                self.rest = rest;
                Ok(Some(Token::RedirectOutput))
            }
            [b'<', ..] => Err("Input redirection is unsupported"),
            [b'|', ..] => Err("Pipes are unsupported"),
            _ => {
                let mut value = Vec::new();
                let mut quoted = false;
                let mut end = input.len();
                for (offset, &byte) in input.iter().enumerate() {
                    if byte == b'"' {
                        quoted = !quoted;
                    } else if !quoted
                        && (byte.is_ascii_whitespace() || matches!(byte, b'>' | b'<' | b'|'))
                    {
                        end = offset;
                        break;
                    } else {
                        value.push(byte);
                    }
                }
                if quoted {
                    return Err("Unclosed double quote");
                }
                let (spelling, rest) = input.split_at(end);
                self.rest = rest;
                Ok(Some(Token::Word(Word {
                    value,
                    spelling,
                    preceding_space,
                })))
            }
        }
    }
}

impl<'a> CommandLine<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Option<Self>, &'static str> {
        let mut lexer = Lexer { rest: bytes };
        let mut words = Vec::new();
        let mut stdout = None;
        let mut ends_in_redirect = false;
        while let Some(token) = lexer.next()? {
            match token {
                Token::Word(word) => {
                    words.push(word);
                    ends_in_redirect = false;
                }
                Token::RedirectOutput => {
                    if stdout.is_some() {
                        return Err("Output redirected more than once");
                    }
                    let Some(Token::Word(target)) = lexer.next()? else {
                        return Err("Expected a device name after redirection");
                    };
                    if target.value.is_empty() {
                        return Err("Expected a device name after redirection");
                    }
                    stdout = Some(target.value);
                    ends_in_redirect = true;
                }
            }
        }
        let mut words = words.into_iter();
        let Some(command) = words.next() else {
            return if stdout.is_some() {
                Err("Expected a command")
            } else {
                Ok(None)
            };
        };
        if command.value.is_empty() {
            return Err("Expected a command");
        }
        Ok(Some(Self {
            name: command.value,
            arguments: words.collect(),
            stdout,
            trailing_space: if ends_in_redirect { b"" } else { lexer.rest },
        }))
    }

    /// ECHO displays the original argument text, including quotes and spacing.
    pub fn argument_text(&self) -> Vec<u8> {
        let mut text = Vec::new();
        if let Some((first, rest)) = self.arguments.split_first() {
            text.extend_from_slice(first.spelling);
            for word in rest {
                text.extend_from_slice(word.preceding_space);
                text.extend_from_slice(word.spelling);
            }
            text.extend_from_slice(self.trailing_space);
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_words_separate_values_from_the_original_echo_text() {
        let line = CommandLine::parse(b" eChO  \"a > b\"\t\xff  ")
            .unwrap()
            .unwrap();
        assert_eq!(line.name, b"eChO");
        assert_eq!(line.arguments[0].value, b"a > b");
        assert_eq!(line.argument_text(), b"\"a > b\"\t\xff  ");
        assert!(line.stdout.is_none());
    }

    #[test]
    fn output_redirection_can_appear_before_between_or_after_words() {
        for bytes in [
            b">\"LPT1\" echo one two".as_slice(),
            b"echo one >>LPT1 two",
            b"echo one two>LPT1",
        ] {
            let line = CommandLine::parse(bytes).unwrap().unwrap();
            assert_eq!(line.name, b"echo");
            assert_eq!(line.argument_text(), b"one two");
            assert_eq!(line.stdout.as_deref(), Some(b"LPT1".as_slice()));
        }
    }

    #[test]
    fn malformed_syntax_is_rejected_as_a_whole() {
        for bytes in [
            b"echo text >".as_slice(),
            b"echo >\"\"",
            b"echo x>PRN>NUL",
            b"echo >>>PRN",
            b"echo \"unterminated",
            b">PRN",
            b"echo a | help",
            b"echo <NUL",
        ] {
            assert!(CommandLine::parse(bytes).is_err(), "{bytes:?}");
        }
        assert!(CommandLine::parse(b" \t ").unwrap().is_none());
    }
}
