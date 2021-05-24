use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

mod syntax;

use crate::syntax::{Command, DataDestination, DataSource, Value, ValueSource, Variable};

fn main() -> anyhow::Result<()> {
    println!("INF: Let's wasmercise!");

    // Vocabulary:
    // assert_exists(source)
    // assert_not_exists(source)
    // assert_value(var)is(val)
    // read(source)to(var)
    // write(val)to(dest)
    //
    // source := file:foo or env:foo
    // dest := file:foo or stm:stdout or stm:stderr
    // var := var:foo
    // val := lit:foo (literal text) or var:foo (contents of variable)

    let real_environment = RealEnvironment::new();
    let mut test_context = TestContext::new(real_environment);

    let script = get_script();
    let result = test_context.process_commands(script);

    let message = match &result {
        Ok(()) => "INF: That's enough wasmercising for now; see you next test!".to_owned(),
        Err(e) => format!("ERR: Failed with {}", e),
    };

    println!("{}", message);

    result
}

fn get_script() -> Vec<String> {
    let command_line_script = get_script_from_command_line();
    if command_line_script.is_empty() {
        get_script_from_environment_variable()
    } else {
        command_line_script
    }
}

fn get_script_from_command_line() -> Vec<String> {
    // TODO: un-hardwire the module file name
    let original_args: Vec<String> = env::args().collect();
    if !original_args.is_empty() && original_args[0] == "wasmerciser.wasm" {
        original_args[1..].to_vec()
    } else {
        original_args
    }
}

fn get_script_from_environment_variable() -> Vec<String> {
    parse_script_from_env_var_value(std::env::var("WASMERCISER_RUN_SCRIPT"))
}

fn parse_script_from_env_var_value(var_value: Result<String, std::env::VarError>) -> Vec<String> {
    match var_value {
        Ok(script_text) => words(script_text),
        Err(_) => vec![],
    }
}

fn words(text: String) -> Vec<String> {
    text.split(' ')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
        .collect()
}

trait Environment {
    fn get_env_var(&self, name: String) -> Result<String, std::env::VarError>;
    fn file_exists(&self, path: &PathBuf) -> bool;
    fn file_content(&self, path: &PathBuf) -> std::io::Result<String>;
    fn write_file(&mut self, path: &PathBuf, content: String) -> std::io::Result<()>;
    fn write_stdout(&mut self, content: String) -> anyhow::Result<()>;
    fn write_stderr(&mut self, content: String) -> anyhow::Result<()>;
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
    fn write_stdout(&mut self, content: String) -> anyhow::Result<()> {
        println!("{}", content);
        Ok(())
    }
    fn write_stderr(&mut self, content: String) -> anyhow::Result<()> {
        eprintln!("{}", content);
        Ok(())
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

    fn process_commands(&mut self, commands: Vec<String>) -> anyhow::Result<()> {
        for command_text in commands {
            self.process_command_text(command_text)?
        }
        Ok(())
    }

    fn process_command_text(&mut self, command_text: String) -> anyhow::Result<()> {
        match Command::parse(command_text.clone()) {
            Ok(command) => self.process_command(command),
            Err(e) => Err(anyhow::anyhow!(
                "Error parsing command '{}': {}",
                command_text,
                e
            )),
        }
    }

    fn process_command(&mut self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::AssertExists(source) => self.assert_exists(source),
            Command::AssertNotExists(source) => self.assert_not_exists(source),
            Command::AssertValue(variable, value) => self.assert_value(variable, value),
            Command::Read(source, destination) => self.read(source, destination),
            Command::Write(source, destination) => self.write(source, destination),
        }
    }

    fn assert_exists(&mut self, source: DataSource) -> anyhow::Result<()> {
        match source {
            DataSource::File(path) => self.assert_file_exists(PathBuf::from(path)),
            DataSource::Env(name) => self.assert_env_var_exists(name),
        }
    }

    fn assert_not_exists(&mut self, source: DataSource) -> anyhow::Result<()> {
        match source {
            DataSource::File(path) => self.assert_file_not_exists(PathBuf::from(path)),
            DataSource::Env(name) => self.assert_env_var_not_exists(name),
        }
    }

    fn assert_value(&mut self, variable: Variable, value: Value) -> anyhow::Result<()> {
        let Variable::Variable(testee_name) = variable;
        let testee_value = self.get_variable(&testee_name)?;
        let against_value = match value {
            Value::Variable(v) => self.get_variable(&v)?,
            Value::Literal(t) => t,
        };
        if testee_value == against_value {
            Ok(())
        } else {
            fail_with(format!(
                "Expected {} to have value '{}' but was '{}'",
                testee_name, testee_value, against_value
            ))
        }
    }

    fn read(&mut self, source: DataSource, destination: Variable) -> anyhow::Result<()> {
        let content = match source {
            DataSource::File(path) => self.file_content(PathBuf::from(path)),
            DataSource::Env(name) => self.env_var_value(name),
        };
        let Variable::Variable(dest_name) = destination;
        self.variables.insert(dest_name, content?);
        Ok(())
    }

    fn write(&mut self, source: ValueSource, destination: DataDestination) -> anyhow::Result<()> {
        let content = match source {
            ValueSource::Variable(name) => self.get_variable(&name)?,
            ValueSource::Literal(text) => text,
            ValueSource::File(path) => self.file_content(PathBuf::from(path))?,
            ValueSource::Env(name) => self.env_var_value(name)?,
        };
        match destination {
            DataDestination::File(path) => self
                .environment
                .write_file(&PathBuf::from(path), content)
                .map_err(anyhow::Error::new),
            DataDestination::StdOut => self.environment.write_stdout(content),
            DataDestination::StdErr => self.environment.write_stderr(content),
        }
    }

    fn assert_file_exists(&self, path: PathBuf) -> anyhow::Result<()> {
        if self.environment.file_exists(&path) {
            Ok(())
        } else {
            fail_with(format!(
                "File {} was expected to exist but did not",
                path.to_string_lossy()
            ))
        }
    }

    fn assert_file_not_exists(&self, path: PathBuf) -> anyhow::Result<()> {
        if self.environment.file_exists(&path) {
            fail_with(format!(
                "File {} was expected NOT to exist but it did exist",
                path.to_string_lossy()
            ))
        } else {
            Ok(())
        }
    }

    fn assert_env_var_exists(&self, name: String) -> anyhow::Result<()> {
        match self.environment.get_env_var(name.clone()) {
            Ok(_) => Ok(()),
            Err(_) => fail_with(format!(
                "Env var {} was supposed to exist but did not",
                name
            )),
        }
    }

    fn assert_env_var_not_exists(&self, name: String) -> anyhow::Result<()> {
        match self.environment.get_env_var(name.clone()) {
            Ok(_) => fail_with(format!(
                "Env var {} was supposed to NOT exist but it did exist",
                name
            )),
            Err(_) => Ok(()),
        }
    }

    fn file_content(&self, path: PathBuf) -> anyhow::Result<String> {
        self.environment.file_content(&path).map_err(|e| {
            anyhow::anyhow!(
                "Error getting content of file {}: {}",
                path.to_string_lossy(),
                e
            )
        })
    }

    fn env_var_value(&self, name: String) -> anyhow::Result<String> {
        self.environment
            .get_env_var(name.clone())
            .map_err(|e| anyhow::anyhow!("Error getting value of env var {}: {}", name, e))
    }

    fn get_variable(&self, name: &str) -> anyhow::Result<String> {
        match self.variables.get(name) {
            Some(s) => Ok(s.to_owned()),
            None => Err(anyhow::anyhow!("Variable {} not set", name)),
        }
    }
}

fn fail_with(message: String) -> anyhow::Result<()> {
    eprintln!("ERR: {}", message);
    Err(anyhow::Error::msg(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

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

        fn write_out(&mut self, name: String, content: String) -> anyhow::Result<()> {
            self.outputs.borrow_mut().push(FakeOutput { name, content });
            Ok(())
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
            Ok(self
                .write_out(path.to_string_lossy().to_string(), content)
                .unwrap())
        }
        fn write_stdout(&mut self, content: String) -> anyhow::Result<()> {
            self.write_out("**stdout**".to_owned(), content)
        }
        fn write_stderr(&mut self, content: String) -> anyhow::Result<()> {
            self.write_out("**stderr**".to_owned(), content)
        }
    }

    fn fake_env() -> FakeEnvironment {
        FakeEnvironment::new()
    }

    #[test]
    fn missing_env_var_means_no_script() {
        let no_env_var = Err(std::env::VarError::NotPresent);
        let result = parse_script_from_env_var_value(no_env_var);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn env_var_with_no_spaces_means_one_command() {
        let env_var = Ok("hello".to_owned());
        let result = parse_script_from_env_var_value(env_var);
        assert_eq!(result.len(), 1);
        assert_eq!("hello".to_owned(), result[0]);
    }

    #[test]
    fn env_var_with_spaces_is_split_into_commands() {
        let env_var = Ok("hello world    and  goodbye  ".to_owned());
        let result = parse_script_from_env_var_value(env_var);
        assert_eq!(result.len(), 4);
        assert_eq!("hello".to_owned(), result[0]);
        assert_eq!("world".to_owned(), result[1]);
        assert_eq!("and".to_owned(), result[2]);
        assert_eq!("goodbye".to_owned(), result[3]);
    }

    #[test]
    fn process_assert_file_exists_ok_when_exists() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_command_text("assert_exists(file:/fizz/buzz.txt)".to_owned());
        assert!(result.is_ok());
    }

    #[test]
    fn process_assert_file_exists_fails_when_doesnt_exist() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_command_text("assert_exists(file:/nope/nope/nope)".to_owned());
        assert!(result.is_err());
    }

    #[test]
    fn process_assert_env_var_exists_ok_when_exists() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_command_text("assert_exists(env:test1)".to_owned());
        assert!(result.is_ok());
    }

    #[test]
    fn process_assert_env_var_exists_fails_when_doesnt_exist() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_command_text("assert_exists(env:nope)".to_owned());
        assert!(result.is_err());
    }

    #[test]
    fn process_assert_value_passes_when_matches_variable() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "read(env:test1a)to(var:e1a)".to_owned(),
            "assert_value(var:e1)is(var:e1a)".to_owned(),
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn process_assert_value_fails_when_does_not_match_variable() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "read(env:test2)to(var:e2)".to_owned(),
            "assert_value(var:e1)is(var:e2)".to_owned(),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn process_assert_value_fails_when_match_variable_does_not_exist() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "assert_value(var:e1)is(var:prodnose)".to_owned(),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn process_assert_value_passes_when_matches_literal() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "assert_value(var:e1)is(lit:one)".to_owned(),
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn process_assert_value_fails_when_does_not_match_literal() {
        let mut context = TestContext::new(fake_env());
        let result = context.process_commands(vec![
            "read(env:test1)to(var:e1)".to_owned(),
            "assert_value(var:e1)is(lit:two)".to_owned(),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn process_read_file_updates_when_exists() {
        let mut context = TestContext::new(fake_env());
        context
            .process_command_text("read(file:/fizz/buzz.txt)to(var:ftest)".to_owned())
            .unwrap();
        assert_eq!(context.variables.get("ftest").unwrap(), "fizzbuzz!");
    }

    #[test]
    fn process_read_env_var_updates_when_exists() {
        let mut context = TestContext::new(fake_env());
        context
            .process_command_text("read(env:test1)to(var:etest)".to_owned())
            .unwrap();
        assert_eq!(context.variables.get("etest").unwrap(), "one");
    }

    #[test]
    fn process_write_file_writes_to_file() {
        let outputs = Rc::new(RefCell::new(Vec::<FakeOutput>::new()));
        let mut context = TestContext::new(FakeEnvironment::over(&outputs));
        context
            .process_commands(vec![
                "read(file:/fizz/buzz.txt)to(var:ftest)".to_owned(),
                "write(var:ftest)to(file:/some/result)".to_owned(),
            ])
            .unwrap();
        assert_eq!(outputs.borrow().len(), 1);
        assert_eq!(outputs.borrow()[0].name, "/some/result");
        assert_eq!(outputs.borrow()[0].content, "fizzbuzz!");
    }

    #[test]
    fn process_write_stdout_writes_to_stdout() {
        let outputs = Rc::new(RefCell::new(Vec::<FakeOutput>::new()));
        let mut context = TestContext::new(FakeEnvironment::over(&outputs));
        context
            .process_commands(vec![
                "read(env:test1)to(var:etest)".to_owned(),
                "write(var:etest)to(stm:stdout)".to_owned(),
            ])
            .unwrap();
        assert_eq!(outputs.borrow().len(), 1);
        assert_eq!(outputs.borrow()[0].name, "**stdout**");
        assert_eq!(outputs.borrow()[0].content, "one");
    }

    #[test]
    fn process_write_stderr_writes_to_stderr() {
        let outputs = Rc::new(RefCell::new(Vec::<FakeOutput>::new()));
        let mut context = TestContext::new(FakeEnvironment::over(&outputs));
        context
            .process_commands(vec![
                "read(env:test1)to(var:etest)".to_owned(),
                "write(var:etest)to(stm:stderr)".to_owned(),
            ])
            .unwrap();
        assert_eq!(outputs.borrow().len(), 1);
        assert_eq!(outputs.borrow()[0].name, "**stderr**");
        assert_eq!(outputs.borrow()[0].content, "one");
    }
}
