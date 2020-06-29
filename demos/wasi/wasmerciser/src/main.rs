use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

mod syntax;

use crate::syntax::{Command, DataDestination, DataSource, Value, Variable};

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
        let Variable::Variable(testee_name) = variable;
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
        let Variable::Variable(dest_name) = destination;
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
