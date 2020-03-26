import { Console, Environ, CommandLine } from "as-wasi";

export function _start(): void {
  Console.log("hello from stdout!");
  Console.error("hello from stderr!");
  let env = new Environ();
  let all_vars = env.all();
  all_vars.forEach(function (val) {
    Console.log(val.key + "=" + val.value);
  });

  let cmd = new CommandLine();
  Console.log("Args are: " + cmd.all().toString())
}
