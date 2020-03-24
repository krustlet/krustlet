use std::env;

fn main() {
    println!("hello from stdout!");
    eprintln!("hello from stderr!");
    for (key, value) in env::vars() {
        println!("{}={}", key, value);
    }
    let args: Vec<String> = env::args().collect();
    println!("Args are: {:?}", args);
}
