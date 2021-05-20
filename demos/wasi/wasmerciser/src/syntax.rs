#[derive(Debug, PartialEq)]
pub enum Command {
    AssertExists(DataSource),
    AssertNotExists(DataSource),
    AssertValue(Variable, Value),
    Read(DataSource, Variable),
    Write(ValueSource, DataDestination),
}

#[derive(Debug, PartialEq)]
pub enum DataSource {
    File(String),
    Env(String),
}

#[derive(Debug, PartialEq)]
pub enum DataDestination {
    File(String),
    StdOut,
    StdErr,
}

#[derive(Debug, PartialEq)]
pub enum Variable {
    Variable(String),
}

#[derive(Debug, PartialEq)]
pub enum Value {
    Variable(String),
    Literal(String),
}

#[derive(Debug, PartialEq)]
pub enum ValueSource {
    Variable(String),
    Literal(String),
    File(String),
    Env(String),
}

impl Command {
    pub fn parse(text: String) -> anyhow::Result<Self> {
        let tokens = CommandToken::parse(text)?;
        match &tokens[0] {
            CommandToken::Bracketed(t) => {
                Err(anyhow::anyhow!("don't put commands in brackets: {}", t))
            }
            CommandToken::Plain(t) => match &t[..] {
                "assert_exists" => Self::parse_assert_exists(&tokens),
                "assert_not_exists" => Self::parse_assert_not_exists(&tokens),
                "assert_value" => Self::parse_assert_value(&tokens),
                "read" => Self::parse_read(&tokens),
                "write" => Self::parse_write(&tokens),
                _ => Err(anyhow::anyhow!("unrecognised command: {}", t)),
            },
        }
    }

    fn parse_assert_exists(tokens: &[CommandToken]) -> anyhow::Result<Self> {
        match &tokens[..] {
            [_, CommandToken::Bracketed(source)] => {
                Ok(Self::AssertExists(DataSource::parse(source.to_string())?))
            }
            _ => Err(anyhow::anyhow!("unexpected assert_exists command syntax")),
        }
    }

    fn parse_assert_not_exists(tokens: &[CommandToken]) -> anyhow::Result<Self> {
        match &tokens[..] {
            [_, CommandToken::Bracketed(source)] => Ok(Self::AssertNotExists(DataSource::parse(
                source.to_string(),
            )?)),
            _ => Err(anyhow::anyhow!(
                "unexpected assert_not_exists command syntax"
            )),
        }
    }

    fn parse_assert_value(tokens: &[CommandToken]) -> anyhow::Result<Self> {
        match &tokens[..] {
            // TODO: enforce that the separator is 'is'
            [_, CommandToken::Bracketed(variable), CommandToken::Plain(_sep), CommandToken::Bracketed(value)] => {
                Ok(Self::AssertValue(
                    Variable::parse(variable.to_string())?,
                    Value::parse(value.to_string())?,
                ))
            }
            _ => Err(anyhow::anyhow!("unexpected assert_value command syntax")),
        }
    }

    fn parse_read(tokens: &[CommandToken]) -> anyhow::Result<Self> {
        match &tokens[..] {
            // TODO: enforce that the separator is 'to'
            [_, CommandToken::Bracketed(source), CommandToken::Plain(_sep), CommandToken::Bracketed(destination)] => {
                Ok(Self::Read(
                    DataSource::parse(source.to_string())?,
                    Variable::parse(destination.to_string())?,
                ))
            }
            _ => Err(anyhow::anyhow!("unexpected read command syntax")),
        }
    }

    fn parse_write(tokens: &[CommandToken]) -> anyhow::Result<Self> {
        match &tokens[..] {
            // TODO: enforce that the separator is 'to'
            [_, CommandToken::Bracketed(value), CommandToken::Plain(_sep), CommandToken::Bracketed(destination)] => {
                Ok(Self::Write(
                    ValueSource::parse(value.to_string())?,
                    DataDestination::parse(destination.to_string())?,
                ))
            }
            _ => Err(anyhow::anyhow!("unexpected write command syntax")),
        }
    }
}

impl DataSource {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["file", f] => Ok(DataSource::File(f.to_string())),
            ["env", e] => Ok(DataSource::Env(e.to_string())),
            _ => Err(anyhow::anyhow!(
                "invalid data source: {} (must be file/env)",
                &text
            )),
        }
    }
}

impl DataDestination {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["file", f] => Ok(DataDestination::File(f.to_string())),
            ["stm", "stdout"] => Ok(DataDestination::StdOut),
            ["stm", "stderr"] => Ok(DataDestination::StdErr),
            _ => Err(anyhow::anyhow!(
                "invalid write destination: {} (must be file/stm)",
                &text
            )),
        }
    }
}

impl Variable {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["var", v] => Ok(Variable::Variable(v.to_string())),
            _ => Err(anyhow::anyhow!(
                "invalid variable reference: {} (must be var)",
                &text
            )),
        }
    }
}

impl Value {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["var", v] => Ok(Self::Variable(v.to_string())),
            ["lit", t] => Ok(Self::Literal(t.to_string())),
            _ => Err(anyhow::anyhow!(
                "invalid value: {} (must be var/lit)",
                &text
            )),
        }
    }
}

impl ValueSource {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["file", f] => Ok(Self::File(f.to_string())),
            ["env", e] => Ok(Self::Env(e.to_string())),
            ["var", v] => Ok(Self::Variable(v.to_string())),
            ["lit", t] => Ok(Self::Literal(t.to_string())),
            _ => Err(anyhow::anyhow!(
                "invalid value source: {} (must be file/env/var/lit)",
                &text
            )),
        }
    }
}

#[derive(Debug, PartialEq)]
enum CommandToken {
    Plain(String),
    Bracketed(String),
}

impl CommandToken {
    fn parse(text: String) -> anyhow::Result<Vec<Self>> {
        if text.starts_with('(') {
            match text.find(')') {
                None => Err(anyhow::anyhow!("unmatched opening parenthesis: {}", text)),
                Some(close_index) => {
                    let bracketed_text = &text[1..close_index];
                    let rest = &text[close_index + 1..];
                    let mut parsed_rest = if !rest.is_empty() {
                        Self::parse(rest.to_string())?
                    } else {
                        Vec::new()
                    };
                    parsed_rest.insert(0, Self::Bracketed(bracketed_text.to_string()));
                    Ok(parsed_rest)
                }
            }
        } else {
            match text.find('(') {
                None => Ok(vec![Self::Plain(text)]),
                Some(open_index) => {
                    let plain_text = &text[0..open_index];
                    let rest = &text[open_index..];
                    let mut parsed_rest = Self::parse(rest.to_string())?;
                    parsed_rest.insert(0, Self::Plain(plain_text.to_string()));
                    Ok(parsed_rest)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // We want to pass literal strings so having something that accepts &str
    // instead of String reduces clutter
    fn parse_command(text: &str) -> anyhow::Result<Command> {
        Command::parse(text.to_owned())
    }
    fn parse_tokens(text: &str) -> anyhow::Result<Vec<CommandToken>> {
        CommandToken::parse(text.to_owned())
    }

    #[test]
    fn tokenise_one_plain() {
        let tokens = parse_tokens("fie").expect("Unexpected parsing error");
        assert_eq!(1, tokens.len());
        assert_eq!(CommandToken::Plain("fie".to_owned()), tokens[0]);
    }

    #[test]
    fn tokenise_one_bracketed() {
        let tokens = parse_tokens("(fie)").expect("Unexpected parsing error");
        assert_eq!(1, tokens.len());
        assert_eq!(CommandToken::Bracketed("fie".to_owned()), tokens[0]);
    }

    #[test]
    fn tokenise_two() {
        let tokens = parse_tokens("assert_exists(file:foo)").expect("Unexpected parsing error");
        assert_eq!(2, tokens.len());
        assert_eq!(CommandToken::Plain("assert_exists".to_owned()), tokens[0]);
        assert_eq!(CommandToken::Bracketed("file:foo".to_owned()), tokens[1]);
    }

    #[test]
    fn tokenise_three() {
        let tokens = parse_tokens("foo(bar)quux").expect("Unexpected parsing error");
        assert_eq!(3, tokens.len());
        assert_eq!(CommandToken::Plain("foo".to_owned()), tokens[0]);
        assert_eq!(CommandToken::Bracketed("bar".to_owned()), tokens[1]);
        assert_eq!(CommandToken::Plain("quux".to_owned()), tokens[2]);
    }

    #[test]
    fn tokenise_four() {
        let tokens = parse_tokens("read(file:foo)to(var:ftext)").expect("Unexpected parsing error");
        assert_eq!(4, tokens.len());
        assert_eq!(CommandToken::Plain("read".to_owned()), tokens[0]);
        assert_eq!(CommandToken::Bracketed("file:foo".to_owned()), tokens[1]);
        assert_eq!(CommandToken::Plain("to".to_owned()), tokens[2]);
        assert_eq!(CommandToken::Bracketed("var:ftext".to_owned()), tokens[3]);
    }

    #[test]
    fn parse_single_assert() {
        let command = parse_command("assert_exists(file:foo)").expect("Unexpected parsing error");
        match command {
            Command::AssertExists(DataSource::File(f)) => {
                assert_eq!(f, "foo", "Expected file 'foo' but got {}", f)
            }
            _ => assert!(false, "Expected AssertExists but got {:?}", command),
        }
    }

    #[test]
    fn parse_single_read() {
        let command =
            parse_command("read(file:foo)to(var:ftext)").expect("Unexpected parsing error");
        match command {
            Command::Read(DataSource::File(f), Variable::Variable(v)) => {
                assert_eq!(f, "foo", "Expected source file 'foo' but got {}", f);
                assert_eq!(v, "ftext", "Expected dest var 'ftext' but got {}", v);
            }
            _ => assert!(false, "Expected Read but got {:?}", command),
        }
    }
}
