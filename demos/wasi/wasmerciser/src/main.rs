use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

fn main() {
    println!("INF: Let's wasmercise!");

    // Vocabulary:
    // assert_exists(source)
    // assert_value(var)is(val)
    // read(source)to(var)
    // write(val)to(dest)
    //
    // source := file:foo or env:foo
    // dest := file:foo or stm:stdout or stm:stderr
    // var := var:foo
    // val := lit:foo (literal text) or var:foo (contents of variable)

    let args: Vec<String> = env::args().skip(1).collect();

    let real_environment = RealEnvironment::new();
    let mut test_context = TestContext::new(real_environment);
    test_context.process_commands(args);

    println!("INF: That's enough wasmercising for now; see you next test!");
}

trait Environment {
    fn get_env_var(&self, name: String) -> Result<String, std::env::VarError>;
    fn file_exists(&self, path: &PathBuf) -> bool;
    fn file_content(&self, path: &PathBuf) -> std::io::Result<String>;
    fn write_file(&mut self, path: &PathBuf, content: String) -> std::io::Result<()>;
    fn write_stdout(&mut self, content: String);
    fn write_stderr(&mut self, content: String);
}

struct RealEnvironment {}

impl RealEnvironment {
    fn new() -> Self {
        Self {}
    }
}

impl Environment for RealEnvironment {
    fn get_env_var(&self, name: String) -> Result<String, std::env::VarError> {
        std::env::var(name)
    }
    fn file_exists(&self, path: &PathBuf) -> bool {
        path.exists()
    }
    fn file_content(&self, path: &PathBuf) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn write_file(&mut self, path: &PathBuf, content: String) -> std::io::Result<()> {
        std::fs::write(path, content)
    }
    fn write_stdout(&mut self, content: String) {
        println!("{}", content)
    }
    fn write_stderr(&mut self, content: String) {
        eprintln!("{}", content)
    }
}

struct TestContext<E: Environment> {
    variables: HashMap<String, String>,
    environment: E,
}

impl<E: Environment> TestContext<E> {
    fn new(environment: E) -> Self {
        TestContext {
            variables: Default::default(),
            environment,
        }
    }

    fn process_commands(&mut self, commands: Vec<String>) {
        for command_text in commands {
            self.process_command_text(command_text);
        }
    }

    fn process_command_text(&mut self, command_text: String) {
        match Command::parse(command_text.clone()) {
            Ok(command) => self.process_command(command),
            Err(e) => panic!("Error parsing command '{}': {}", command_text, e),
        }
    }

    fn process_command(&mut self, command: Command) {
        match command {
            Command::AssertExists(source) => self.assert_exists(source),
            Command::AssertValue(variable, value) => self.assert_value(variable, value),
            Command::Read(source, destination) => self.read(source, destination),
            Command::Write(value, destination) => self.write(value, destination),
        }
    }

    fn assert_exists(&mut self, source: DataSource) {
        match source {
            DataSource::File(path) => self.assert_file_exists(PathBuf::from(path)),
            DataSource::Env(name) => self.assert_env_var_exists(name),
        }
    }

    fn assert_value(&mut self, variable: Variable, value: Value) {
        let Variable(testee_name) = variable;
        let testee_value = self.variables.get(&testee_name).unwrap().to_owned();
        let against_value = match value {
            Value::Variable(v) => self.variables.get(&v).unwrap().to_owned(),
            Value::Literal(t) => t,
        };
        if testee_value != against_value {
            fail_with(format!(
                "Expected {} to have value '{}' but was '{}'",
                testee_name, testee_value, against_value
            ));
        }
    }

    fn read(&mut self, source: DataSource, destination: Variable) {
        let content = match source {
            DataSource::File(path) => self.file_content(PathBuf::from(path)),
            DataSource::Env(name) => self.env_var_value(name),
        };
        let Variable(dest_name) = destination;
        self.variables.insert(dest_name, content);
    }

    fn write(&mut self, value: Value, destination: DataDestination) {
        let content = match value {
            Value::Variable(name) => self.variables.get(&name).unwrap().to_owned(),
            Value::Literal(text) => text,
        };
        match destination {
            DataDestination::File(path) => self
                .environment
                .write_file(&PathBuf::from(path), content)
                .unwrap(),
            DataDestination::StdOut => self.environment.write_stdout(content),
            DataDestination::StdErr => self.environment.write_stderr(content),
        };
    }

    fn assert_file_exists(&self, path: PathBuf) {
        if !self.environment.file_exists(&path) {
            fail_with(format!(
                "File {} was expected to exist but did not",
                path.to_string_lossy()
            ));
        }
    }

    fn assert_env_var_exists(&self, name: String) {
        match self.environment.get_env_var(name.clone()) {
            Ok(_) => (),
            Err(_) => fail_with(format!(
                "Env var {} was supposed to exist but did not",
                name
            )),
        }
    }

    fn file_content(&self, path: PathBuf) -> String {
        self.environment.file_content(&path).unwrap()
    }

    fn env_var_value(&self, name: String) -> String {
        self.environment.get_env_var(name).unwrap()
    }
}

fn fail_with(message: String) {
    eprintln!("ERR: {}", message);
    panic!(message);
}

#[derive(Debug, PartialEq)]
enum Command {
    AssertExists(DataSource),
    AssertValue(Variable, Value),
    Read(DataSource, Variable),
    Write(Value, DataDestination),
}

impl Command {
    fn parse(text: String) -> anyhow::Result<Self> {
        let tokens = CommandToken::parse(text)?;
        match &tokens[0] {
            CommandToken::Bracketed(t) => {
                Err(anyhow::anyhow!("don't put commands in brackets: {}", t))
            }
            CommandToken::Plain(t) => match &t[..] {
                "assert_exists" => Self::parse_assert_exists(&tokens),
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

    fn parse_assert_value(tokens: &[CommandToken]) -> anyhow::Result<Self> {
        match &tokens[..] {
            // TODO: enforce that the separator is 'is'
            [_, CommandToken::Bracketed(variable), CommandToken::Plain(_sep), CommandToken::Bracketed(value)] => {
                Ok(Self::AssertValue(
                    Variable::parse(variable.to_string())?,
                    Value::parse(value.to_string())?,
                ))
            }
            _ => Err(anyhow::anyhow!("unexpected read command syntax")),
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
                    Value::parse(value.to_string())?,
                    DataDestination::parse(destination.to_string())?,
                ))
            }
            _ => Err(anyhow::anyhow!("unexpected read command syntax")),
        }
    }
}

#[derive(Debug, PartialEq)]
enum DataSource {
    File(String),
    Env(String),
}

#[derive(Debug, PartialEq)]
enum DataDestination {
    File(String),
    StdOut,
    StdErr,
}

#[derive(Debug, PartialEq)]
struct Variable(String);

#[derive(Debug, PartialEq)]
enum Value {
    Variable(String),
    Literal(String),
}

impl DataSource {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["file", f] => Ok(DataSource::File(f.to_string())),
            ["env", e] => Ok(DataSource::Env(e.to_string())),
            _ => Err(anyhow::anyhow!("invalid data source")),
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
            _ => Err(anyhow::anyhow!("invalid write destination")),
        }
    }
}

impl Variable {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["var", v] => Ok(Variable(v.to_string())),
            _ => Err(anyhow::anyhow!("invalid variable reference")),
        }
    }
}

impl Value {
    fn parse(text: String) -> anyhow::Result<Self> {
        let bits: Vec<&str> = text.split(':').collect();
        match bits[..] {
            ["var", v] => Ok(Self::Variable(v.to_string())),
            ["lit", t] => Ok(Self::Literal(t.to_string())),
            _ => Err(anyhow::anyhow!("invalid value")),
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
    use std::cell::RefCell;
    use std::rc::Rc;

    // We want to pass literal strings so having something that accepts &str
    // instead of String reduces clutter
    fn parse_command(text: &str) -> anyhow::Result<Command> {
        Command::parse(text.to_owned())
    }
    fn parse_tokens(text: &str) -> anyhow::Result<Vec<CommandToken>> {
        CommandToken::parse(text.to_owned())
    }

    struct FakeOutput {
        pub name: String,
        pub content: String,
    }

    struct FakeEnvironment {
        pub outputs: Rc<RefCell<Vec<FakeOutput>>>,
    }

    impl FakeEnvironment {
        fn new() -> Self {
            FakeEnvironment {
                outputs: Rc::new(RefCell::new(vec![])),
            }
        }

        fn over(outputs: &Rc<RefCell<Vec<FakeOutput>>>) -> Self {
            FakeEnvironment {
                outputs: outputs.clone(),
            }
        }

        fn write_out(&mut self, name: String, content: String) {
            self.outputs.borrow_mut().push(FakeOutput { name, content });
        }
    }

    impl Environment for FakeEnvironment {
        fn get_env_var(&self, name: String) -> Result<String, std::env::VarError> {
            match &name[..] {
                "test1" => Ok("one".to_owned()),
                "test1a" => Ok("one".to_owned()),
                "test2" => Ok("two".to_owned()),
                _ => Err(std::env::VarError::NotPresent),
            }
        }
        fn file_exists(&self, path: &PathBuf) -> bool {
            path.to_string_lossy() == "/fizz/buzz.txt"
        }
        fn file_content(&self, path: &PathBuf) -> std::io::Result<String> {
            if path.to_string_lossy() == "/fizz/buzz.txt" {
                Ok("fizzbuzz!".to_owned())
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            }
        }
        fn write_file(&mut self, path: &PathBuf, content: String) -> std::io::Result<()> {
            Ok(self.write_out(path.to_string_lossy().to_string(), content))
        }
        fn write_stdout(&mut self, content: String) {
            self.write_out("**stdout**".to_owned(), content)
        }
        fn write_stderr(&mut self, content: String) {
            self.write_out("**stderr**".to_owned(), content)
        }
    }

    fn fake_env() -> FakeEnvironment {
        FakeEnvironment::new()
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
            Command::Read(DataSource::File(f), Variable(v)) => {
                assert_eq!(f, "foo", "Expected source file 'foo' but got {}", f);
                assert_eq!(v, "ftext", "Expected dest var 'ftext' but got {}", v);
            }
            _ => assert!(false, "Expected Read but got {:?}", command),
        }
    }

    #[test]
    fn process_assert_file_exists_ok_when_exists() {
        let mut context = TestContext::new(fake_env());
        context.process_command_text("assert_exists(file:/fizz/buzz.txt)".to_owned());
    }

    #[test]
    #[should_panic]
    fn process_assert_file_exists_panics_when_doesnt_exist() {
        let mut context = TestContext::new(fake_env());
        context.process_command_text("assert_exists(file:/nope/nope/nope)".to_owned());
    }

    #[test]
    fn process_assert_env_var_exists_ok_when_exists() {
        let mut context = TestContext::new(fake_env());
        context.process_command_text("assert_exists(env:test1)".to_owned());
    }

    #[test]
    #[should_panic]
    fn process_assert_env_var_exists_panics_when_doesnt_exist() {
        let mut context = TestContext::new(fake_env());
        context.process_command_text("assert_exists(env:nope)".to_owned());
    }

    #[test]
    fn process_assert_value_passes_when_matches_variable() {
        let mut context = TestContext::new(fake_env());
        context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "read(env:test1a)to(var:e1a)".to_owned(),
            "assert_value(var:e1)is(var:e1a)".to_owned(),
        ]);
    }

    #[test]
    #[should_panic]
    fn process_assert_value_panics_when_does_not_match_variable() {
        let mut context = TestContext::new(fake_env());
        context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "read(env:test2)to(var:e2)".to_owned(),
            "assert_value(var:e1)is(var:e2)".to_owned(),
        ]);
    }

    #[test]
    #[should_panic]
    fn process_assert_value_panics_when_match_variable_does_not_exist() {
        let mut context = TestContext::new(fake_env());
        context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "assert_value(var:e1)is(var:prodnose)".to_owned(),
        ]);
    }

    #[test]
    fn process_assert_value_passes_when_matches_literal() {
        let mut context = TestContext::new(fake_env());
        context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "assert_value(var:e1)is(lit:one)".to_owned(),
        ]);
    }

    #[test]
    #[should_panic]
    fn process_assert_value_panics_when_does_not_match_literal() {
        let mut context = TestContext::new(fake_env());
        context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "assert_value(var:e1)is(lit:two)".to_owned(),
        ]);
    }

    #[test]
    fn process_read_file_updates_when_exists() {
        let mut context = TestContext::new(fake_env());
        context.process_command_text("read(file:/fizz/buzz.txt)to(var:ftest)".to_owned());
        assert_eq!(context.variables.get("ftest").unwrap(), "fizzbuzz!");
    }

    #[test]
    fn process_read_env_var_updates_when_exists() {
        let mut context = TestContext::new(fake_env());
        context.process_command_text("read(env:test1)to(var:etest)".to_owned());
        assert_eq!(context.variables.get("etest").unwrap(), "one");
    }

    #[test]
    fn process_write_file_writes_to_file() {
        let outputs = Rc::new(RefCell::new(Vec::<FakeOutput>::new()));
        let mut context = TestContext::new(FakeEnvironment::over(&outputs));
        context.process_commands(vec![
            "read(file:/fizz/buzz.txt)to(var:ftest)".to_owned(),
            "write(var:ftest)to(file:/some/result)".to_owned(),
        ]);
        assert_eq!(outputs.borrow().len(), 1);
        assert_eq!(outputs.borrow()[0].name, "/some/result");
        assert_eq!(outputs.borrow()[0].content, "fizzbuzz!");
    }

    #[test]
    fn process_write_stdout_writes_to_stdout() {
        let outputs = Rc::new(RefCell::new(Vec::<FakeOutput>::new()));
        let mut context = TestContext::new(FakeEnvironment::over(&outputs));
        context.process_commands(vec![
            "read(env:test1)to(var:etest)".to_owned(),
            "write(var:etest)to(stm:stdout)".to_owned(),
        ]);
        assert_eq!(outputs.borrow().len(), 1);
        assert_eq!(outputs.borrow()[0].name, "**stdout**");
        assert_eq!(outputs.borrow()[0].content, "one");
    }

    #[test]
    fn process_write_stderr_writes_to_stderr() {
        let outputs = Rc::new(RefCell::new(Vec::<FakeOutput>::new()));
        let mut context = TestContext::new(FakeEnvironment::over(&outputs));
        context.process_commands(vec![
            "read(env:test1)to(var:etest)".to_owned(),
            "write(var:etest)to(stm:stderr)".to_owned(),
        ]);
        assert_eq!(outputs.borrow().len(), 1);
        assert_eq!(outputs.borrow()[0].name, "**stderr**");
        assert_eq!(outputs.borrow()[0].content, "one");
    }
}
